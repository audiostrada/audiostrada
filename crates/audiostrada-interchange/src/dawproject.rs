//! DAWproject (version 1.0): a ZIP holding `project.xml`, `metadata.xml`, the audio files and
//! the plugins' states. Tracks, buses, effect returns, clips (audio, notes, clips of clips),
//! warped audio, fades, volume, pan, mute, sends, plugins with their states, volume and pan
//! automation, tempo, time signature and markers go both ways.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use audiostrada_core::automation::{AutomationTarget, CurveShape};
use audiostrada_core::file::ProjectPaths;
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::tempo::{TempoMap, TempoPoint, TimeSignature};
use audiostrada_core::{
    Clip, FadeShape, Insert, Marker, Note, Project, TrackId, TrackKind, TrackSend, base64,
};
use roxmltree::Node;

use crate::zip::{Archive, Writer};
use crate::{Build, Warning, db, gain, vstpreset};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    Beats,
    Seconds,
}

fn unit_of(node: Node, attr: &str) -> Option<Unit> {
    match node.attribute(attr)? {
        "beats" => Some(Unit::Beats),
        "seconds" => Some(Unit::Seconds),
        _ => None,
    }
}

fn num(node: Node, attr: &str) -> Option<f64> {
    node.attribute(attr)?
        .trim()
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite())
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| c.has_tag_name(name))
}

/// A timeline the times inside are counted on: its unit, and where its zero falls.
#[derive(Clone, Copy, Debug)]
struct Frame {
    unit: Unit,
    seconds: f64,
    beats: f64,
}

struct Clock {
    tempo: TempoMap,
    rate: u32,
}

impl Clock {
    fn seconds(&self, frame: Frame, t: f64) -> f64 {
        match frame.unit {
            Unit::Seconds => frame.seconds + t,
            Unit::Beats => self.tempo.beat_to_seconds(frame.beats + t),
        }
    }

    fn samples(&self, seconds: f64) -> u64 {
        (seconds.max(0.0) * f64::from(self.rate)).round() as u64
    }

    /// The frame whose time `at` (in `unit`) falls at `seconds` on the timeline.
    fn frame_at(&self, unit: Unit, seconds: f64, at: f64) -> Frame {
        let beat = self.tempo.seconds_to_beat(seconds);
        Frame {
            unit,
            seconds: seconds - at,
            beats: beat - at,
        }
    }
}

/// What an automation lane drives.
#[derive(Clone, Copy)]
enum Param {
    Volume(Option<TrackId>),
    Pan(Option<TrackId>),
    Mute(Option<TrackId>),
}

struct Import<'a> {
    build: Build,
    zip: &'a Archive,
    dir: PathBuf,
    clock: Clock,
    /// Channel and track ids of the file: the track (None: the master).
    channels: BTreeMap<String, Option<TrackId>>,
    params: BTreeMap<String, Param>,
}

pub(crate) fn import(
    path: &Path,
    paths: &ProjectPaths,
    name: &str,
) -> Result<(Project, Vec<Warning>), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let zip = Archive::read(data)?;
    let xml = zip.get("project.xml").ok_or("no project.xml in the file")?;
    let xml = String::from_utf8(xml).map_err(|_| "project.xml is not UTF-8".to_owned())?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    if !root.has_tag_name("Project") {
        return Err("not a DAWproject".into());
    }
    let title = zip
        .get("metadata.xml")
        .and_then(|m| String::from_utf8(m).ok())
        .and_then(|m| {
            let d = roxmltree::Document::parse(&m).ok()?;
            let t = child(d.root_element(), "Title")?.text()?.trim().to_owned();
            (!t.is_empty()).then_some(t)
        });

    let mut bpm = 120.0;
    let (mut num_, mut den) = (4, 4);
    if let Some(t) = child(root, "Transport") {
        if let Some(v) = child(t, "Tempo").and_then(|t| num(t, "value")) {
            bpm = v;
        }
        if let Some(s) = child(t, "TimeSignature") {
            num_ = s
                .attribute("numerator")
                .and_then(|v| v.parse().ok())
                .unwrap_or(4);
            den = s
                .attribute("denominator")
                .and_then(|v| v.parse().ok())
                .unwrap_or(4);
        }
    }
    let arrangement = child(root, "Arrangement");
    let mut tempos = vec![TempoPoint { beat: 0.0, bpm }];
    let mut signatures = vec![TimeSignature {
        bar: 0,
        numerator: num_,
        denominator: den,
    }];
    if let Some(a) = arrangement {
        if let Some(points) = child(a, "TempoAutomation") {
            let first = TempoMap::constant(bpm, num_, den);
            for p in points.children().filter(|p| p.has_tag_name("RealPoint")) {
                if let (Some(t), Some(v)) = (num(p, "time"), num(p, "value")) {
                    let beat = match unit_of(points, "timeUnit") {
                        Some(Unit::Seconds) => first.seconds_to_beat(t),
                        _ => t,
                    };
                    tempos.push(TempoPoint { beat, bpm: v });
                }
            }
        }
        if let Some(points) = child(a, "TimeSignatureAutomation") {
            let per_bar = f64::from(num_) * 4.0 / f64::from(den);
            for p in points
                .children()
                .filter(|p| p.has_tag_name("TimeSignaturePoint"))
            {
                if let (Some(t), Some(n), Some(d)) = (
                    num(p, "time"),
                    p.attribute("numerator").and_then(|v| v.parse().ok()),
                    p.attribute("denominator").and_then(|v| v.parse().ok()),
                ) {
                    signatures.push(TimeSignature {
                        bar: (t / per_bar).round().max(0.0) as u32,
                        numerator: n,
                        denominator: d,
                    });
                }
            }
        }
    }
    // A point at 0 from the automation replaces the transport's.
    tempos.reverse();
    signatures.reverse();
    let tempo = TempoMap::new(tempos, signatures);

    let mut build = Build::new(paths, title.as_deref().unwrap_or(name), 48_000);
    build.project.tempo = tempo.clone();
    let mut im = Import {
        build,
        zip: &zip,
        dir: path.parent().unwrap_or(Path::new(".")).to_path_buf(),
        clock: Clock {
            tempo,
            rate: 48_000,
        },
        channels: BTreeMap::new(),
        params: BTreeMap::new(),
    };
    let mut sends = Vec::new();
    if let Some(s) = child(root, "Structure") {
        for node in s.children().filter(Node::is_element) {
            im.structure(node, None, &mut sends);
        }
    }
    for (from, to, level, pre) in sends {
        if let Some(Some(target)) = im.channels.get(&to).copied()
            && let Some(t) = im.build.project.track_mut(from)
        {
            t.sends.push(TrackSend {
                target,
                gain_db: level,
                pre_fader: pre,
                mute: false,
            });
        }
    }
    if let Some(a) = arrangement {
        let unit = child(a, "Lanes")
            .and_then(|l| unit_of(l, "timeUnit"))
            .unwrap_or(Unit::Beats);
        let top = Frame {
            unit,
            seconds: 0.0,
            beats: 0.0,
        };
        for node in a.children().filter(|n| n.has_tag_name("Lanes")) {
            im.lanes(node, None, top);
        }
        if let Some(markers) = child(a, "Markers") {
            let frame = Frame {
                unit: unit_of(markers, "timeUnit").unwrap_or(unit),
                ..top
            };
            for m in markers.children().filter(|m| m.has_tag_name("Marker")) {
                if let Some(t) = num(m, "time") {
                    let position = im.clock.samples(im.clock.seconds(frame, t));
                    let id = im.build.project.new_marker_id();
                    im.build.project.markers.push(Marker {
                        id,
                        position,
                        name: m.attribute("name").unwrap_or_default().to_owned(),
                    });
                }
            }
        }
    }
    let Import { build, .. } = im;
    Ok((build.project, build.warnings))
}

impl Import<'_> {
    /// A track of the structure, its channel and its own tracks; or a lone channel.
    fn structure(
        &mut self,
        node: Node,
        parent: Option<TrackId>,
        sends: &mut Vec<(TrackId, String, f32, bool)>,
    ) {
        if node.has_tag_name("Channel") {
            if node.attribute("role") == Some("master") {
                self.channel(node, None, sends);
            }
            return;
        }
        if !node.has_tag_name("Track") {
            return;
        }
        let channel = child(node, "Channel");
        let role = channel
            .and_then(|c| c.attribute("role"))
            .unwrap_or("regular");
        if role == "master" {
            if let Some(c) = channel {
                self.channel(c, None, sends);
            }
            return;
        }
        let content = node.attribute("contentType").unwrap_or("");
        let folder = content.split_whitespace().any(|c| c == "tracks")
            || node.children().any(|c| c.has_tag_name("Track"));
        let name = node.attribute("name").unwrap_or("Track");
        let id = self.build.project.add_track(name);
        if let Some(t) = self.build.project.track_mut(id) {
            t.parent = parent;
            if folder || role == "effect" || role == "submix" {
                t.kind = TrackKind::Bus;
            } else if role == "vca" {
                t.kind = TrackKind::Vca;
            }
            if let Some(color) = node.attribute("color").and_then(color) {
                t.color = color;
            }
            t.stereo = channel.and_then(|c| c.attribute("audioChannels")) != Some("1");
        }
        if let Some(xml_id) = node.attribute("id") {
            self.channels.insert(xml_id.to_owned(), Some(id));
        }
        if let Some(c) = channel {
            self.channel(c, Some(id), sends);
        }
        let notes = content.split_whitespace().any(|c| c == "notes");
        let played = |i: &Insert| {
            ["clap:", "vst3:", "vst2:", "au:"]
                .iter()
                .any(|p| i.effect.starts_with(p))
        };
        if notes
            && self
                .build
                .project
                .track(id)
                .is_some_and(|t| !t.inserts.iter().any(played))
            && let Some(synth) = routing::add_insert(
                &mut self.build.project,
                Channel::Track(id),
                "synth",
                BTreeMap::new(),
            )
            && let Some(t) = self.build.project.track_mut(id)
            && let Some(at) = t.inserts.iter().position(|i| i.id == synth)
        {
            let synth = t.inserts.remove(at);
            t.inserts.insert(0, synth);
        }
        let bus = (folder || role == "effect" || role == "submix").then_some(id);
        for sub in node.children().filter(|c| c.has_tag_name("Track")) {
            self.structure(sub, bus.or(parent), sends);
        }
    }

    /// A channel's fader, pan, mute, solo, sends and devices.
    fn channel(
        &mut self,
        node: Node,
        track: Option<TrackId>,
        sends: &mut Vec<(TrackId, String, f32, bool)>,
    ) {
        if let Some(xml_id) = node.attribute("id") {
            self.channels.insert(xml_id.to_owned(), track);
        }
        let volume = child(node, "Volume").map(|v| (level(v), v.attribute("id")));
        let pan = child(node, "Pan").map(|p| (pan_value(p), p.attribute("id")));
        let mute =
            child(node, "Mute").map(|m| (m.attribute("value") == Some("true"), m.attribute("id")));
        for (id, param) in [
            (volume.and_then(|v| v.1), Param::Volume(track)),
            (pan.and_then(|p| p.1), Param::Pan(track)),
            (mute.and_then(|m| m.1), Param::Mute(track)),
        ] {
            if let Some(id) = id {
                self.params.insert(id.to_owned(), param);
            }
        }
        match track {
            None => {
                if let Some((v, _)) = volume {
                    self.build.project.master.gain_db = v;
                }
            }
            Some(id) => {
                if let Some(t) = self.build.project.track_mut(id) {
                    if let Some((v, _)) = volume {
                        t.gain_db = v;
                    }
                    if let Some((p, _)) = pan {
                        t.pan = p;
                    }
                    if let Some((m, _)) = mute {
                        t.mute = m;
                    }
                    t.solo = node.attribute("solo") == Some("true");
                }
                if let Some(s) = child(node, "Sends") {
                    for send in s.children().filter(|c| c.has_tag_name("Send")) {
                        if let Some(to) = send.attribute("destination") {
                            let level = child(send, "Volume").map_or(0.0, level);
                            sends.push((
                                id,
                                to.to_owned(),
                                level,
                                send.attribute("type") == Some("pre"),
                            ));
                        }
                    }
                }
            }
        }
        if let Some(devices) = child(node, "Devices") {
            let channel = track.map_or(Channel::Master, Channel::Track);
            for d in devices.children().filter(Node::is_element) {
                self.device(d, channel);
            }
        }
    }

    fn device(&mut self, d: Node, channel: Channel) {
        let name = d
            .attribute("deviceName")
            .or(d.attribute("name"))
            .unwrap_or("?")
            .to_owned();
        let enabled = child(d, "Enabled").is_none_or(|e| e.attribute("value") != Some("false"));
        let state = child(d, "State").and_then(|s| {
            let p = s.attribute("path")?;
            if s.attribute("external") == Some("true") {
                std::fs::read(self.dir.join(p)).ok()
            } else {
                self.zip.get(p)
            }
        });
        let id = d.attribute("deviceID").unwrap_or("").to_owned();
        let (effect, state, params) = match d.tag_name().name() {
            "ClapPlugin" => (format!("clap:{id}"), state, BTreeMap::new()),
            "Vst3Plugin" => {
                let state = state.and_then(|s| vstpreset::from_preset(&s));
                (
                    format!("vst3:{}", id.to_ascii_uppercase()),
                    state,
                    BTreeMap::new(),
                )
            }
            "Vst2Plugin" => (format!("vst2:{id}"), None, BTreeMap::new()),
            "AuPlugin" => (format!("au:{id}"), None, BTreeMap::new()),
            "Equalizer" => ("eq".to_owned(), None, equalizer(d)),
            "Compressor" => ("compressor".to_owned(), None, dynamics(d, &COMPRESSOR)),
            "NoiseGate" => ("gate".to_owned(), None, dynamics(d, &GATE)),
            "Limiter" => ("limiter".to_owned(), None, dynamics(d, &LIMITER)),
            _ => {
                self.build.warn(Warning::Left(name));
                return;
            }
        };
        if effect.ends_with(':') {
            self.build.warn(Warning::Left(name));
            return;
        }
        if effect.contains(':') && state.is_none() {
            self.build.warn(Warning::NoSettings(name));
        }
        if let Some(insert) = routing::add_insert(&mut self.build.project, channel, &effect, params)
            && let Some(i) = routing::find_insert_mut(&mut self.build.project, channel, insert)
        {
            i.bypass = !enabled;
            i.state = state.map(|s| base64::encode(&s));
        }
    }

    /// A lane of the arrangement: its clips, automation and inner lanes.
    fn lanes(&mut self, node: Node, track: Option<Option<TrackId>>, frame: Frame) {
        let frame = Frame {
            unit: unit_of(node, "timeUnit").unwrap_or(frame.unit),
            ..frame
        };
        let track = node
            .attribute("track")
            .and_then(|t| self.channels.get(t).copied())
            .or(track);
        for c in node.children().filter(Node::is_element) {
            match c.tag_name().name() {
                "Lanes" => self.lanes(c, track, frame),
                "Clips" => {
                    if let Some(Some(t)) = track {
                        let frame = Frame {
                            unit: unit_of(c, "timeUnit").unwrap_or(frame.unit),
                            ..frame
                        };
                        for clip in c.children().filter(|n| n.has_tag_name("Clip")) {
                            self.clip(clip, t, frame, None);
                        }
                    }
                }
                "Points" => self.points(c, frame),
                _ => {}
            }
        }
    }

    /// A clip, cut to `window` (seconds) when inside another clip.
    fn clip(&mut self, node: Node, track: TrackId, frame: Frame, window: Option<(f64, f64)>) {
        let Some(time) = num(node, "time") else {
            return;
        };
        let duration = num(node, "duration").unwrap_or(0.0);
        let mut start = self.clock.seconds(frame, time);
        let mut end = self.clock.seconds(frame, time + duration);
        let play_start = num(node, "playStart").unwrap_or(0.0);
        let content_unit = unit_of(node, "contentTimeUnit").unwrap_or(frame.unit);
        let content = self.clock.frame_at(content_unit, start, play_start);
        if let Some((a, b)) = window {
            start = start.max(a);
            end = end.min(b);
        }
        if end <= start {
            return;
        }
        let fade_unit = unit_of(node, "fadeTimeUnit").unwrap_or(frame.unit);
        let fade = |t: f64| match fade_unit {
            Unit::Seconds => t,
            Unit::Beats => {
                t * 60.0
                    / self
                        .clock
                        .tempo
                        .bpm_at_beat(self.clock.tempo.seconds_to_beat(start))
            }
        };
        let fades = (
            fade(num(node, "fadeInTime").unwrap_or(0.0)),
            fade(num(node, "fadeOutTime").unwrap_or(0.0)),
        );
        let muted = node.attribute("enable") == Some("false");
        let name = node.attribute("name").map(str::to_owned);
        // An alias clip plays the content of another, named by its id.
        let referenced = || {
            let id = node.attribute("reference")?;
            node.document()
                .descendants()
                .find(|n| n.attribute("id") == Some(id))
        };
        let Some(body) = node.children().find(Node::is_element).or_else(referenced) else {
            return;
        };
        let cut = start - self.clock.seconds(frame, time);
        let play = match content_unit {
            Unit::Seconds => play_start,
            Unit::Beats => {
                play_start * 60.0
                    / self
                        .clock
                        .tempo
                        .bpm_at_beat(self.clock.tempo.seconds_to_beat(start))
            }
        };
        let placed = Placed {
            start,
            end,
            fades,
            muted,
            name,
            file_at_start: play + cut,
        };
        match body.tag_name().name() {
            "Audio" => self.audio(body, track, &placed, None),
            "Warps" => {
                if let Some(audio) = body.children().find(|n| n.has_tag_name("Audio")) {
                    let warps: Vec<(f64, f64)> = body
                        .children()
                        .filter(|n| n.has_tag_name("Warp"))
                        .filter_map(|w| Some((num(w, "time")?, num(w, "contentTime")?)))
                        .collect();
                    let frame = Frame {
                        unit: unit_of(body, "timeUnit").unwrap_or(content.unit),
                        ..content
                    };
                    let seconds: Vec<(f64, f64)> = warps
                        .iter()
                        .map(|&(t, c)| (self.clock.seconds(frame, t), c))
                        .collect();
                    self.audio(audio, track, &placed, Some(seconds));
                }
            }
            "Notes" => self.notes(body, track, &placed, content),
            "Clips" => {
                let frame = Frame {
                    unit: unit_of(body, "timeUnit").unwrap_or(content.unit),
                    ..content
                };
                for c in body.children().filter(|n| n.has_tag_name("Clip")) {
                    self.clip(c, track, frame, Some((start, end)));
                }
            }
            "Lanes" => {
                for clips in body.descendants().filter(|n| n.has_tag_name("Clips")) {
                    for c in clips.children().filter(|n| n.has_tag_name("Clip")) {
                        self.clip(c, track, content, Some((start, end)));
                    }
                }
            }
            _ => {}
        }
    }

    fn audio(
        &mut self,
        audio: Node,
        track: TrackId,
        placed: &Placed,
        warps: Option<Vec<(f64, f64)>>,
    ) {
        let Some(file) = child(audio, "File").and_then(|f| {
            let p = f.attribute("path")?;
            Some((p.to_owned(), f.attribute("external") == Some("true")))
        }) else {
            return;
        };
        let path = if file.1 {
            let p = PathBuf::from(&file.0);
            if p.is_absolute() { p } else { self.dir.join(p) }
        } else {
            let key = format!("zip:{}", file.0);
            match self
                .build
                .sources
                .iter()
                .find(|(k, _)| k.to_string_lossy() == key)
            {
                Some((_, &id)) => {
                    let path = self.build.project.sources[&id].path.clone();
                    self.build.paths.resolve(&path)
                }
                None => {
                    let Some(data) = self.zip.get(&file.0) else {
                        self.build.warn(Warning::Missing(PathBuf::from(&file.0)));
                        return;
                    };
                    let Some(stored) = self.build.store(&file.0, &data) else {
                        return;
                    };
                    if let Some(id) = self.build.source(&stored) {
                        self.build.sources.insert(PathBuf::from(key), id);
                    }
                    stored
                }
            }
        };
        let Some(source) = self.build.source(&path) else {
            return;
        };
        let rate = f64::from(self.clock.rate);
        // Seconds of the file at the clip's start, and how much longer the clip plays it.
        let (file_start, stretch) = match warps.as_deref() {
            Some([a, .., b]) if (b.0 - a.0).abs() > 1e-9 && (b.1 - a.1).abs() > 1e-9 => {
                let ratio = (b.0 - a.0) / (b.1 - a.1);
                (a.1 + (placed.start - a.0) / ratio, ratio)
            }
            _ => (placed.file_at_start, 1.0),
        };
        let stretch = if (stretch - 1.0).abs() < 1e-4 {
            1.0
        } else {
            stretch
        };
        let start = self.clock.samples(placed.start);
        let end = self.clock.samples(placed.end);
        let id = self.build.project.new_clip_id();
        let name = placed.name.clone().unwrap_or_else(|| {
            path.file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
        });
        let clip = Clip {
            id,
            source,
            name,
            start,
            offset: (file_start.max(0.0) * stretch * rate).round() as u64,
            length: end.saturating_sub(start).max(1),
            gain_db: 0.0,
            fade_in: (placed.fades.0 * rate) as u64,
            fade_out: (placed.fades.1 * rate) as u64,
            fade_shape: FadeShape::default(),
            fade_out_shape: None,
            muted: placed.muted,
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
        if let Some(t) = self.build.project.track_mut(track) {
            t.clips.push(clip);
            t.clips.sort_by_key(|c| (c.start, c.id));
        }
    }

    fn notes(&mut self, body: Node, track: TrackId, placed: &Placed, content: Frame) {
        let frame = Frame {
            unit: unit_of(body, "timeUnit").unwrap_or(content.unit),
            ..content
        };
        let start = self.clock.samples(placed.start);
        let end = self.clock.samples(placed.end);
        let mut notes = Vec::new();
        for n in body.children().filter(|n| n.has_tag_name("Note")) {
            let (Some(t), Some(d), Some(key)) = (
                num(n, "time"),
                num(n, "duration"),
                n.attribute("key").and_then(|k| k.parse::<u8>().ok()),
            ) else {
                continue;
            };
            let a = self.clock.samples(self.clock.seconds(frame, t));
            let b = self.clock.samples(self.clock.seconds(frame, t + d));
            if a < start || a >= end {
                continue;
            }
            notes.push(Note {
                start: a - start,
                length: b.min(end).saturating_sub(a).max(1),
                key: key.min(127),
                velocity: (num(n, "vel").unwrap_or(0.8) * 127.0)
                    .round()
                    .clamp(1.0, 127.0) as u8,
                channel: n
                    .attribute("channel")
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(0),
                ..Note::default()
            });
        }
        if let Some(clip) = self
            .build
            .project
            .add_midi_clip(track, start, end - start, notes)
            && let Some(t) = self.build.project.track_mut(track)
            && let Some(c) = t.clips.iter_mut().find(|c| c.id == clip)
        {
            c.muted = placed.muted;
            if let Some(name) = &placed.name {
                c.name.clone_from(name);
            }
        }
    }

    fn points(&mut self, node: Node, frame: Frame) {
        let frame = Frame {
            unit: unit_of(node, "timeUnit").unwrap_or(frame.unit),
            ..frame
        };
        let Some(param) = child(node, "Target")
            .and_then(|t| t.attribute("parameter"))
            .and_then(|p| self.params.get(p).copied())
        else {
            return;
        };
        let unit = node.attribute("unit").unwrap_or("linear");
        let mut points = Vec::new();
        for p in node.children().filter(Node::is_element) {
            let Some(t) = num(p, "time") else {
                continue;
            };
            let time = self.clock.samples(self.clock.seconds(frame, t));
            let step = p.attribute("interpolation") == Some("hold");
            let value = match (p.tag_name().name(), param) {
                ("RealPoint", Param::Volume(_)) => {
                    let v = num(p, "value").unwrap_or(1.0);
                    if unit == "decibel" { v as f32 } else { db(v) }
                }
                ("RealPoint", Param::Pan(_)) => (num(p, "value").unwrap_or(0.5) * 2.0 - 1.0) as f32,
                ("BoolPoint", Param::Mute(_)) => {
                    f32::from(u8::from(p.attribute("value") == Some("true")))
                }
                _ => continue,
            };
            points.push((time, value, step || matches!(param, Param::Mute(_))));
        }
        let (track, target) = match param {
            Param::Volume(t) => (t, AutomationTarget::Gain),
            Param::Pan(t) => (t, AutomationTarget::Pan),
            Param::Mute(t) => (t, AutomationTarget::Mute),
        };
        self.build.automate(track, target, points);
    }
}

struct Placed {
    start: f64,
    end: f64,
    fades: (f64, f64),
    muted: bool,
    name: Option<String>,
    /// Seconds into the content where the clip starts.
    file_at_start: f64,
}

/// A volume in decibels, from its unit.
fn level(node: Node) -> f32 {
    let v = num(node, "value").unwrap_or(1.0);
    match node.attribute("unit") {
        Some("decibel") => v as f32,
        _ => db(v),
    }
}

fn pan_value(node: Node) -> f32 {
    let v = num(node, "value").unwrap_or(0.5);
    match node.attribute("unit") {
        Some("percent") => (v / 50.0 - 1.0).clamp(-1.0, 1.0) as f32,
        _ => (v * 2.0 - 1.0).clamp(-1.0, 1.0) as f32,
    }
}

fn color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    let v = u32::from_str_radix(hex.get(..6)?, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// A RealParameter in the unit AudioStrada counts it in.
fn real(node: Node, name: &str) -> Option<f32> {
    let p = child(node, name)?;
    let v = num(p, "value")?;
    Some(match p.attribute("unit") {
        Some("seconds") => v * 1000.0,
        Some("linear") if name.contains("Gain") || name == "Threshold" => f64::from(db(v)),
        _ => v,
    } as f32)
}

const COMPRESSOR: [(&str, &str); 5] = [
    ("Threshold", "threshold"),
    ("Ratio", "ratio"),
    ("Attack", "attack"),
    ("Release", "release"),
    ("OutputGain", "makeup"),
];
const GATE: [(&str, &str); 5] = [
    ("Threshold", "threshold"),
    ("Range", "range"),
    ("Attack", "attack"),
    ("Release", "release"),
    ("Ratio", "ratio"),
];
const LIMITER: [(&str, &str); 5] = [
    ("Threshold", "ceiling"),
    ("InputGain", "input"),
    ("Release", "release"),
    ("Attack", "lookahead"),
    ("OutputGain", ""),
];

fn dynamics(node: Node, map: &[(&str, &str)]) -> BTreeMap<String, f32> {
    map.iter()
        .filter(|(_, ours)| !ours.is_empty())
        .filter_map(|&(theirs, ours)| Some((ours.to_owned(), real(node, theirs)?)))
        .collect()
}

/// DAWproject's band types, in the order of Strada-EQ's.
const BANDS: [&str; 7] = [
    "bell",
    "lowShelf",
    "highShelf",
    "highPass",
    "lowPass",
    "notch",
    "bandPass",
];

fn equalizer(node: Node) -> BTreeMap<String, f32> {
    let mut params = BTreeMap::new();
    for (i, band) in node
        .children()
        .filter(|c| c.has_tag_name("Band"))
        .take(16)
        .enumerate()
    {
        let n = i + 1;
        let Some(kind) = band
            .attribute("type")
            .and_then(|t| BANDS.iter().position(|b| *b == t))
        else {
            continue;
        };
        params.insert(format!("b{n}_used"), 1.0);
        params.insert(format!("b{n}_type"), kind as f32);
        if let Some(f) = real(band, "Freq") {
            params.insert(format!("b{n}_freq"), f);
        }
        if let Some(g) = real(band, "Gain") {
            params.insert(format!("b{n}_gain"), g);
        }
        if let Some(q) = real(band, "Q") {
            params.insert(format!("b{n}_q"), q);
        }
        if let Some(order) = band.attribute("order").and_then(|o| o.parse::<u32>().ok()) {
            params.insert(format!("b{n}_slope"), order.clamp(1, 4) as f32 - 1.0);
        }
        if child(band, "Enabled").is_some_and(|e| e.attribute("value") == Some("false")) {
            params.insert(format!("b{n}_bypass"), 1.0);
        }
    }
    params
}

/// Writes `project` (media under `paths`) as a DAWproject: audio files and plugin states
/// inside. Built-in effects of AudioStrada stay out: no other DAW has them.
pub fn export(
    project: &Project,
    paths: Option<&ProjectPaths>,
    out: &Path,
) -> Result<Vec<Warning>, String> {
    let mut warnings: Vec<Warning> = Vec::new();
    let mut warn = |w: Warning| {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    };
    let rate = f64::from(project.sample_rate.max(1));
    let secs = |s: u64| s as f64 / rate;
    let mut next = 0u64;
    let mut id = || {
        next += 1;
        format!("id{next}")
    };
    let mut files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    let mut audio_names: BTreeMap<audiostrada_core::SourceId, String> = BTreeMap::new();

    let master_id = id();
    let mut channel_ids: BTreeMap<TrackId, String> = BTreeMap::new();
    let mut track_ids: BTreeMap<TrackId, String> = BTreeMap::new();
    for t in &project.tracks {
        channel_ids.insert(t.id, id());
        track_ids.insert(t.id, id());
    }
    let mut params: BTreeMap<(Option<TrackId>, &str), String> = BTreeMap::new();

    let mut devices = |inserts: &[Insert],
                       x: &mut String,
                       files: &mut Vec<(String, Vec<u8>, bool)>,
                       id: &mut dyn FnMut() -> String| {
        x.push_str("<Devices>");
        for i in inserts {
            let Some((format, plugin)) = i.effect.split_once(':') else {
                warn(Warning::Left(i.effect.clone()));
                continue;
            };
            let (tag, ext) = match format {
                "clap" => ("ClapPlugin", "clap-preset"),
                "vst3" => ("Vst3Plugin", "vstpreset"),
                _ => {
                    warn(Warning::Left(i.effect.clone()));
                    continue;
                }
            };
            let device = id();
            x.push_str(&format!(
                r#"<{tag} id="{device}" deviceID="{}" deviceName="{}" deviceRole="audioFX" loaded="true">"#,
                esc(plugin),
                esc(plugin)
            ));
            x.push_str(&format!(
                r#"<Enabled id="{}" value="{}"/>"#,
                id(),
                !i.bypass
            ));
            let state = i.state.as_deref().and_then(base64::decode).and_then(|s| {
                if format == "vst3" {
                    vstpreset::to_preset(plugin, &s)
                } else {
                    Some(s)
                }
            });
            match state {
                Some(s) => {
                    let path = format!("plugins/{device}.{ext}");
                    x.push_str(&format!(r#"<State path="{path}"/>"#));
                    files.push((path, s, true));
                }
                None => warn(Warning::NoSettings(plugin.to_owned())),
            }
            x.push_str(&format!("</{tag}>"));
        }
        x.push_str("</Devices>");
    };

    let mut structure = String::new();
    // Tracks in order, those of a bus inside it.
    fn children(project: &Project, parent: Option<TrackId>) -> Vec<&audiostrada_core::Track> {
        project
            .tracks
            .iter()
            .filter(|t| t.parent == parent)
            .map(|t| t.as_ref())
            .collect()
    }
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn write_track(
        project: &Project,
        t: &audiostrada_core::Track,
        x: &mut String,
        ids: (&BTreeMap<TrackId, String>, &BTreeMap<TrackId, String>, &str),
        params: &mut BTreeMap<(Option<TrackId>, &'static str), String>,
        id: &mut dyn FnMut() -> String,
        devices: &mut dyn FnMut(&[Insert], &mut String, &mut dyn FnMut() -> String),
    ) {
        let (channel_ids, track_ids, master) = ids;
        let bus = t.kind == TrackKind::Bus;
        let midi = t.clips.iter().any(Clip::is_midi);
        let content = if bus {
            "tracks"
        } else if midi {
            "notes"
        } else {
            "audio"
        };
        let [r, g, b] = t.color;
        x.push_str(&format!(
            r##"<Track contentType="{content}" loaded="true" id="{}" name="{}" color="#{r:02x}{g:02x}{b:02x}">"##,
            track_ids[&t.id],
            esc(&t.name)
        ));
        let destination = t
            .parent
            .map_or(master.to_owned(), |p| channel_ids[&p].clone());
        let role = match t.kind {
            TrackKind::Bus => "submix",
            TrackKind::Vca => "vca",
            TrackKind::Audio => "regular",
        };
        x.push_str(&format!(
            r#"<Channel audioChannels="{}" destination="{destination}" role="{role}" solo="{}" id="{}">"#,
            if t.stereo || bus { 2 } else { 1 },
            t.solo,
            channel_ids[&t.id]
        ));
        devices(&t.inserts, x, id);
        let (mute, pan, volume) = (id(), id(), id());
        x.push_str(&format!(
            r#"<Mute value="{}" id="{mute}" name="Mute"/>"#,
            t.mute
        ));
        x.push_str(&format!(
            r#"<Pan max="1.0" min="0.0" unit="normalized" value="{}" id="{pan}" name="Pan"/>"#,
            (t.pan + 1.0) / 2.0
        ));
        if !t.sends.is_empty() {
            x.push_str("<Sends>");
            for s in &t.sends {
                if let Some(to) = channel_ids.get(&s.target) {
                    x.push_str(&format!(
                        r#"<Send id="{}" destination="{to}" type="{}"><Volume max="2.0" min="0.0" unit="linear" value="{}" id="{}"/></Send>"#,
                        id(),
                        if s.pre_fader { "pre" } else { "post" },
                        if s.mute { 0.0 } else { gain(s.gain_db) },
                        id()
                    ));
                }
            }
            x.push_str("</Sends>");
        }
        x.push_str(&format!(
            r#"<Volume max="2.0" min="0.0" unit="linear" value="{}" id="{volume}" name="Volume"/>"#,
            gain(t.gain_db)
        ));
        x.push_str("</Channel>");
        params.insert((Some(t.id), "volume"), volume);
        params.insert((Some(t.id), "pan"), pan);
        params.insert((Some(t.id), "mute"), mute);
        for c in children(project, Some(t.id)) {
            write_track(
                project,
                c,
                x,
                (channel_ids, track_ids, master),
                params,
                id,
                devices,
            );
        }
        x.push_str("</Track>");
    }

    let mut device_files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    {
        let mut dev = |inserts: &[Insert], x: &mut String, id: &mut dyn FnMut() -> String| {
            devices(inserts, x, &mut device_files, id);
        };
        for t in children(project, None) {
            write_track(
                project,
                t,
                &mut structure,
                (&channel_ids, &track_ids, &master_id),
                &mut params,
                &mut id,
                &mut dev,
            );
        }
        structure.push_str(&format!(r#"<Channel audioChannels="2" role="master" solo="false" id="{master_id}" name="Master">"#));
        dev(&project.master.inserts, &mut structure, &mut id);
        let volume = id();
        structure.push_str(&format!(
            r#"<Volume max="2.0" min="0.0" unit="linear" value="{}" id="{volume}" name="Volume"/>"#,
            gain(project.master.gain_db)
        ));
        structure.push_str("</Channel>");
        params.insert((None, "volume"), volume);
    }
    files.append(&mut device_files);

    let mut lanes = String::new();
    for t in &project.tracks {
        let mut inner = String::new();
        let mut clips = String::new();
        for c in &t.clips {
            let (start, length) = (secs(c.start), secs(c.length));
            let common = format!(
                r#"time="{start}" duration="{length}" playStart="{}" fadeTimeUnit="seconds" fadeInTime="{}" fadeOutTime="{}" name="{}" enable="{}""#,
                if c.is_midi() {
                    0.0
                } else {
                    secs(c.offset) / c.stretch
                },
                secs(c.fade_in),
                secs(c.fade_out),
                esc(&c.name),
                !c.muted
            );
            if let Some(notes) = &c.notes {
                clips.push_str(&format!(
                    r#"<Clip {common} contentTimeUnit="seconds"><Notes id="{}">"#,
                    id()
                ));
                for n in notes.iter() {
                    clips.push_str(&format!(
                        r#"<Note time="{}" duration="{}" channel="{}" key="{}" vel="{}" rel="{}"/>"#,
                        secs(n.start),
                        secs(n.length),
                        n.channel,
                        n.key,
                        f64::from(n.velocity) / 127.0,
                        f64::from(n.velocity) / 127.0
                    ));
                }
                clips.push_str("</Notes></Clip>");
                continue;
            }
            let Some(source) = project.sources.get(&c.source) else {
                continue;
            };
            let name = match audio_names.get(&c.source) {
                Some(n) => n.clone(),
                None => {
                    let path =
                        paths.map_or_else(|| source.path.clone(), |p| p.resolve(&source.path));
                    let Ok(data) = std::fs::read(&path) else {
                        warn(Warning::Missing(path));
                        continue;
                    };
                    let file = path.file_name().map_or_else(
                        || "audio.wav".to_owned(),
                        |f| f.to_string_lossy().into_owned(),
                    );
                    let mut name = format!("audio/{file}");
                    let mut n = 2;
                    while files.iter().any(|f| f.0 == name) {
                        name = format!("audio/{n}-{file}");
                        n += 1;
                    }
                    files.push((name.clone(), data, false));
                    audio_names.insert(c.source, name.clone());
                    name
                }
            };
            let file_seconds = source.frames as f64 / f64::from(source.sample_rate.max(1));
            let audio = format!(
                r#"<Audio id="{}" algorithm="stretch" channels="{}" duration="{file_seconds}" sampleRate="{}"><File path="{}"/></Audio>"#,
                id(),
                source.channels,
                source.sample_rate,
                esc(&name)
            );
            if (c.stretch - 1.0).abs() > 1e-6 {
                clips.push_str(&format!(
                    r#"<Clip {common} contentTimeUnit="seconds"><Warps id="{}" contentTimeUnit="seconds" timeUnit="seconds">{audio}<Warp time="0" contentTime="0"/><Warp time="{}" contentTime="{file_seconds}"/></Warps></Clip>"#,
                    id(),
                    file_seconds * c.stretch
                ));
            } else {
                clips.push_str(&format!(
                    r#"<Clip {common} contentTimeUnit="seconds">{audio}</Clip>"#
                ));
            }
        }
        if !clips.is_empty() {
            inner.push_str(&format!(r#"<Clips id="{}">{clips}</Clips>"#, id()));
        }
        inner.push_str(&points(&t.automation, Some(t.id), &params, &mut id, secs));
        if !inner.is_empty() {
            lanes.push_str(&format!(
                r#"<Lanes track="{}" id="{}">{inner}</Lanes>"#,
                track_ids[&t.id],
                id()
            ));
        }
    }
    lanes.push_str(&points(
        &project.master.automation,
        None,
        &params,
        &mut id,
        secs,
    ));

    let tempo = &project.tempo;
    let first = tempo.tempos().first().map_or(120.0, |t| t.bpm);
    let sig = tempo
        .signatures()
        .first()
        .copied()
        .unwrap_or(TimeSignature {
            bar: 0,
            numerator: 4,
            denominator: 4,
        });
    let mut x = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    x.push_str(r#"<Project version="1.0">"#);
    x.push_str(&format!(
        r#"<Application name="AudioStrada" version="{}"/>"#,
        env!("CARGO_PKG_VERSION")
    ));
    x.push_str(&format!(
        r#"<Transport><Tempo max="999.0" min="20.0" unit="bpm" value="{first}" id="{}" name="Tempo"/><TimeSignature denominator="{}" numerator="{}" id="{}"/></Transport>"#,
        id(),
        sig.denominator,
        sig.numerator,
        id()
    ));
    x.push_str(&format!("<Structure>{structure}</Structure>"));
    x.push_str(&format!(
        r#"<Arrangement id="{}"><Lanes timeUnit="seconds" id="{}">{lanes}</Lanes>"#,
        id(),
        id()
    ));
    if !project.markers.is_empty() {
        x.push_str(&format!(r#"<Markers id="{}" timeUnit="seconds">"#, id()));
        for m in &project.markers {
            x.push_str(&format!(
                r#"<Marker time="{}" name="{}"/>"#,
                secs(m.position),
                esc(&m.name)
            ));
        }
        x.push_str("</Markers>");
    }
    if tempo.tempos().len() > 1 {
        x.push_str(&format!(
            r#"<TempoAutomation id="{}" unit="bpm"><Target/>"#,
            id()
        ));
        for t in tempo.tempos() {
            x.push_str(&format!(
                r#"<RealPoint time="{}" value="{}" interpolation="hold"/>"#,
                t.beat, t.bpm
            ));
        }
        x.push_str("</TempoAutomation>");
    }
    x.push_str("</Arrangement><Scenes/></Project>");

    let meta = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><MetaData><Title>{}</Title></MetaData>"#,
        esc(&project.name)
    );
    let file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut zip = Writer::new(std::io::BufWriter::new(file));
    let io = |e: std::io::Error| e.to_string();
    zip.add("project.xml", x.as_bytes(), true).map_err(io)?;
    zip.add("metadata.xml", meta.as_bytes(), true).map_err(io)?;
    for (name, data, compress) in &files {
        zip.add(name, data, *compress).map_err(io)?;
    }
    zip.finish().map_err(io)?;
    Ok(warnings)
}

/// Volume, pan and mute automation as Points of their channel's parameters.
fn points(
    lanes: &[audiostrada_core::automation::AutomationLane],
    track: Option<TrackId>,
    params: &BTreeMap<(Option<TrackId>, &str), String>,
    id: &mut dyn FnMut() -> String,
    secs: impl Fn(u64) -> f64,
) -> String {
    let mut x = String::new();
    for lane in lanes.iter().filter(|l| !l.points.is_empty()) {
        let (key, unit) = match lane.target {
            AutomationTarget::Gain => ("volume", "linear"),
            AutomationTarget::Pan => ("pan", "normalized"),
            AutomationTarget::Mute => ("mute", ""),
            _ => continue,
        };
        let Some(param) = params.get(&(track, key)) else {
            continue;
        };
        let unit = if unit.is_empty() {
            String::new()
        } else {
            format!(r#" unit="{unit}""#)
        };
        x.push_str(&format!(
            r#"<Points id="{}"{unit}><Target parameter="{param}"/>"#,
            id()
        ));
        for p in &lane.points {
            let time = secs(p.time);
            let hold = if p.shape == CurveShape::Step {
                "hold"
            } else {
                "linear"
            };
            match lane.target {
                AutomationTarget::Gain => x.push_str(&format!(
                    r#"<RealPoint time="{time}" value="{}" interpolation="{hold}"/>"#,
                    gain(p.value)
                )),
                AutomationTarget::Pan => x.push_str(&format!(
                    r#"<RealPoint time="{time}" value="{}" interpolation="{hold}"/>"#,
                    (p.value + 1.0) / 2.0
                )),
                _ => x.push_str(&format!(
                    r#"<BoolPoint time="{time}" value="{}"/>"#,
                    p.value >= 0.5
                )),
            }
        }
        x.push_str("</Points>");
    }
    x
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
