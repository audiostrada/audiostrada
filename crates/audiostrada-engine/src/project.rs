//! Keeps the engine graph in sync with a project.
//!
//! Every channel (track or master) is an input gain stage where its sources sum, its insert
//! chain, then a strip (fader, pan, mute, solo, meter). Tracks feed their folder or the master
//! and their sends; the master feeds a loudness meter, then hardware outputs 1-2. The routing
//! edges are recomputed from the project and diffed against the graph.
//!
//! The model stores timeline positions at the project rate; they are scaled to the engine rate.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use audiostrada_core::automation::{AutomationMode, AutomationTarget};
use audiostrada_core::base64;
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::{
    Insert as InsertModel, InsertId, Monitoring, Project, ProjectPaths, TrackId, TrackKind,
};
use audiostrada_dsp::db_to_gain;
use audiostrada_plugins::PluginInstance;
use audiostrada_plugins::scan::Database;

use crate::automation::{AutomationSet, Conversion, Lane as AutomationLane};
use crate::loudness::{LoudnessMeter, LoudnessReadout};
use crate::media::MediaPool;
use crate::meter::MeterReadout;
use crate::nodes::{
    Clip, ClipPlayer, Gain, Insert, InsertControls, LevelControls, MidiClip, MidiControls,
    MidiPlayer, Monitor, PlayerControls, PluginControls, PluginNode, Sequence, SoloBus, Strip,
    StripControls,
};
use crate::record::{Recorder, TakeTarget};
use crate::{Engine, EngineConfig, EngineProcessor, HARDWARE_CHANNELS, NodeId, Port, SharedParam};
use audiostrada_midi::live::LiveMidi;

#[derive(Debug)]
struct PluginSlot {
    /// Automation slot of each automated parameter.
    automation: HashMap<u32, usize>,
    instance: Box<dyn PluginInstance>,
    controls: PluginControls,
    /// Parameter values last exchanged with the plugin.
    known: HashMap<u32, f64>,
    latency: u32,
}

#[derive(Debug)]
enum InsertKind {
    Builtin(InsertControls),
    Plugin(PluginSlot),
}

#[derive(Debug)]
struct InsertNodes {
    id: InsertId,
    effect: String,
    node: NodeId,
    kind: InsertKind,
    /// First insert of a track: it receives the track's MIDI.
    first: bool,
    /// Files the effect was built with.
    files: std::collections::BTreeMap<String, PathBuf>,
    /// Latency of the effect, and the one told to the graph (none while bypassed).
    latency: u32,
    declared: u32,
}

/// Plugin instances and what they need besides the graph.
#[derive(Debug)]
struct Plugins {
    database: Database,
    /// Sample rate and block size plugins are activated with.
    format: (u32, usize),
    /// State to restore when an instance is recreated (restart, format change).
    pending_state: HashMap<InsertId, Vec<u8>>,
    failed: HashMap<InsertId, String>,
    /// AudioStrada's shortcuts pressed in plugin windows (X keysym, modifiers).
    keys: Vec<(u32, u16)>,
    /// Instances waiting for their audio side to be dropped before deactivation.
    retired: Vec<Box<dyn PluginInstance>>,
    feedback: Vec<(Channel, InsertId, u32, f64)>,
    resync: bool,
    /// Rendering faster or slower than real time (export).
    offline: bool,
    /// Where relative effect files are found.
    paths: Option<ProjectPaths>,
    /// What each built-in effect file turned out to be, or why it failed.
    files: HashMap<(InsertId, String), Result<String, String>>,
    /// Low latency mode: inserts adding more than this many samples are bypassed.
    low_latency: Option<u32>,
    /// Inserts the low latency mode bypasses.
    forced: Vec<InsertId>,
}

impl Plugins {
    /// Removes an insert slot, keeping a plugin's state for its next instance.
    fn retire(&mut self, slot: InsertNodes, keep_state: bool) -> NodeId {
        if let InsertKind::Plugin(mut plugin) = slot.kind {
            if keep_state && let Some(state) = plugin.instance.save_state() {
                self.pending_state.insert(slot.id, state);
            }
            plugin.instance.close_gui();
            self.retired.push(plugin.instance);
        }
        slot.node
    }

    fn create(
        &mut self,
        engine: &mut Engine,
        insert: &InsertModel,
        midi: Option<MidiPlayer>,
    ) -> Result<InsertNodes, String> {
        let descriptor = self
            .database
            .find(&insert.effect)
            .cloned()
            .ok_or_else(|| format!("plugin {} is not installed", insert.effect))?;
        // Until activated: a plugin that crashes AudioStrada meanwhile is refused next time.
        let _loading = audiostrada_plugins::guard::loading(&descriptor);
        let mut instance = audiostrada_plugins::instantiate(&descriptor)?;
        let state = self
            .pending_state
            .remove(&insert.id)
            .or_else(|| insert.state.as_deref().and_then(base64::decode));
        if let Some(state) = state {
            instance.load_state(&state);
        }
        let (rate, max) = self.format;
        let mut processor = instance.activate(rate, max)?;
        processor.set_offline(self.offline);
        let latency = instance.latency();
        let (node, controls) = PluginNode::new(processor);
        let first = midi.is_some();
        let node = engine.add_node(&descriptor.name, node.with_midi(midi), 2, 2);
        let _ = engine.set_latency(node, latency);
        Ok(InsertNodes {
            id: insert.id,
            effect: insert.effect.clone(),
            node,
            first,
            files: insert.files.clone(),
            latency,
            declared: latency,
            kind: InsertKind::Plugin(PluginSlot {
                instance,
                controls,
                known: HashMap::new(),
                automation: HashMap::new(),
                latency,
            }),
        })
    }
}

/// Input stage, inserts and strip of a track or of the master.
#[derive(Debug)]
struct ChannelNodes {
    input: NodeId,
    input_gain: SharedParam,
    inserts: Vec<InsertNodes>,
    strip: NodeId,
    controls: StripControls,
    /// MIDI of the first insert (tracks only).
    midi: Option<MidiControls>,
}

#[derive(Debug)]
struct SendNodes {
    target: TrackId,
    pre_fader: bool,
    node: NodeId,
    gain: SharedParam,
}

/// The MIDI output of a track playing an external instrument.
#[derive(Debug)]
struct MidiOutNodes {
    node: NodeId,
    controls: MidiControls,
    port: String,
    out: LiveMidi,
}

#[derive(Debug)]
struct TrackNodes {
    channel: ChannelNodes,
    player: NodeId,
    player_controls: PlayerControls,
    /// What the player currently holds: model clips, media generation and engine rate.
    sent: Option<(Vec<audiostrada_core::Clip>, u64, u32)>,
    monitor: NodeId,
    monitor_gain: SharedParam,
    /// Level of the track's input, monitored or not.
    input_meter: MeterReadout,
    monitoring: Monitoring,
    armed: bool,
    /// Led by an instrument: armed for live MIDI, it never hears the audio inputs.
    instrument: bool,
    midi_out: Option<MidiOutNodes>,
    /// Hardware inputs feeding the monitor.
    monitor_inputs: (usize, usize),
    sends: Vec<SendNodes>,
}

#[derive(Debug)]
pub struct ProjectPlayer {
    engine: Engine,
    solo: SoloBus,
    master: ChannelNodes,
    loudness: LoudnessReadout,
    tracks: HashMap<TrackId, TrackNodes>,
    /// Routing edges managed by `sync`.
    edges: HashSet<(Port, Port)>,
    project_rate: u32,
    loop_range: Option<(u64, u64)>,
    tempo: Option<audiostrada_core::tempo::TempoMap>,
    metronome: crate::nodes::MetronomeControls,
    /// The click is on, and whether it waits for a recording; whether one is going on.
    click: (bool, bool),
    recording_click: bool,
    /// Master inserts heard live only.
    reference: crate::nodes::ReferenceControls,
    /// The reference audio given to the engine.
    reference_audio: Option<Arc<audiostrada_media::AudioData>>,
    monitor: ChannelNodes,
    /// Armed tracks hear their input only while stopped or recording (Pro Tools style).
    auto_monitoring: bool,
    recording: bool,
    preview: crate::nodes::PreviewControls,
    plugins: Plugins,
    /// Override flags of the automated controls, kept across automation updates.
    overrides: HashMap<(Channel, AutomationTarget), Arc<AtomicBool>>,
    /// Live MIDI input, read by the instruments of armed tracks.
    live: LiveMidi,
}

/// Absolute path of every source of `project`, plus the stretched or transposed variants
/// its clips play.
pub fn media_paths(project: &Project, paths: Option<&ProjectPaths>) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = project
        .sources
        .values()
        .map(|s| resolve(paths, &s.path))
        .chain(project.reference.iter().map(|r| resolve(paths, &r.path)))
        .collect();
    for clip in project.tracks.iter().flat_map(|t| t.clips.iter()) {
        if let (Some(variant), Some(source)) = (clip.variant(), project.sources.get(&clip.source)) {
            let path = audiostrada_core::variant_path(&resolve(paths, &source.path), variant);
            if !all.contains(&path) {
                all.push(path);
            }
        }
    }
    all
}

/// Media path a clip plays: its source, or the stretched variant of it.
pub fn clip_media(
    project: &Project,
    paths: Option<&ProjectPaths>,
    clip: &audiostrada_core::Clip,
) -> Option<PathBuf> {
    let source = project.sources.get(&clip.source)?;
    let path = resolve(paths, &source.path);
    Some(match clip.variant() {
        Some(variant) => audiostrada_core::variant_path(&path, variant),
        None => path,
    })
}

/// Reads or decodes a file for a built-in effect and hands it over.
fn load_effect_file(
    effect: &mut dyn audiostrada_dsp::Effect,
    info: &audiostrada_dsp::EffectInfo,
    slot: &str,
    path: &Path,
) -> Result<String, String> {
    let file = info
        .files
        .iter()
        .find(|f| f.id == slot)
        .ok_or_else(|| format!("unknown file slot {slot}"))?;
    let kind = file.kind;
    let data = match kind {
        audiostrada_dsp::FileKind::Data => {
            audiostrada_dsp::FileData::Data(std::fs::read(path).map_err(|e| e.to_string())?)
        }
        audiostrada_dsp::FileKind::Audio => {
            let audio = audiostrada_media::load(path).map_err(|e| e.to_string())?;
            audiostrada_dsp::FileData::Audio {
                channels: (0..audio.channels())
                    .map(|c| audio.channel(c).to_vec())
                    .collect(),
                rate: audio.sample_rate(),
            }
        }
        audiostrada_dsp::FileKind::Pack => {
            let mut files: Vec<PathBuf> = if path.is_dir() {
                std::fs::read_dir(path)
                    .map_err(|e| e.to_string())?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().is_some_and(|e| {
                            let e = e.to_string_lossy().to_lowercase();
                            file.extensions.iter().any(|x| *x == e)
                        })
                    })
                    .collect()
            } else {
                vec![path.to_path_buf()]
            };
            files.sort();
            if files.is_empty() {
                return Err(format!(
                    "no {} file in {}",
                    file.extensions.join("/"),
                    path.display()
                ));
            }
            audiostrada_dsp::FileData::Pack(
                files
                    .iter()
                    .map(std::fs::read)
                    .collect::<Result<_, _>>()
                    .map_err(|e| e.to_string())?,
            )
        }
    };
    effect.load(slot, data)
}

fn resolve(paths: Option<&ProjectPaths>, media: &Path) -> PathBuf {
    paths.map_or_else(|| media.to_path_buf(), |p| p.resolve(media))
}

fn connect_stereo(engine: &mut Engine, from: NodeId, to: NodeId) {
    for ch in 0..2 {
        engine
            .connect(Port::new(from, ch), Port::new(to, ch))
            .expect("stereo nodes have two channels");
    }
}

impl ChannelNodes {
    fn new(engine: &mut Engine, solo: &SoloBus, name: &str, master: bool) -> Self {
        let (input, input_gain) = Gain::new(1.0, None);
        let input = engine.add_node(&format!("{name} input"), input, 2, 2);
        let (strip, controls) = Strip::new(solo, master);
        let strip = engine.add_node(name, strip, 2, 2);
        Self {
            input,
            input_gain,
            inserts: Vec::new(),
            strip,
            midi: None,
            controls,
        }
    }

    /// Last node before the fader, where pre-fader sends are taken.
    fn pre_fader(&self) -> NodeId {
        self.inserts.last().map_or(self.input, |i| i.node)
    }

    fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        [self.input, self.strip]
            .into_iter()
            .chain(self.inserts.iter().map(|i| i.node))
    }
}

/// Lanes that drive a parameter: enabled, with points, on a track whose mode reads them.
fn automated(project: &Project) -> HashSet<(Channel, AutomationTarget)> {
    let mut out = HashSet::new();
    let active =
        |l: &&audiostrada_core::automation::AutomationLane| l.enabled && !l.points.is_empty();
    for lane in project.master.automation.iter().filter(active) {
        out.insert((Channel::Master, lane.target.clone()));
    }
    for track in project
        .tracks
        .iter()
        .filter(|t| t.automation_mode != AutomationMode::Off)
    {
        for lane in track.automation.iter().filter(active) {
            out.insert((Channel::Track(track.id), lane.target.clone()));
        }
    }
    out
}

/// Parameter values of an insert in its effect's order.
fn param_values(insert: &InsertModel, info: &audiostrada_dsp::EffectInfo) -> Vec<f32> {
    info.params
        .iter()
        .map(|p| insert.params.get(p.id).copied().unwrap_or(p.default))
        .collect()
}

impl ProjectPlayer {
    pub fn new(config: EngineConfig) -> Self {
        let mut engine = Engine::new(config);
        let solo = SoloBus::default();
        let master = ChannelNodes::new(&mut engine, &solo, "Master", true);
        let (meter, loudness) = LoudnessMeter::new();
        let meter = engine.add_node("Loudness", meter, 2, 2);
        connect_stereo(&mut engine, master.strip, meter);
        let out = engine.graph().hardware_output();
        // The reference track and the monitor inserts sit after the meters, on the way to the
        // outputs.
        let (reference, reference_controls) = crate::nodes::Reference::new();
        let reference = engine.add_node("Reference", reference, 2, 2);
        connect_stereo(&mut engine, meter, reference);
        let monitor = ChannelNodes::new(&mut engine, &solo, "Monitor", true);
        connect_stereo(&mut engine, reference, monitor.input);
        connect_stereo(&mut engine, monitor.strip, out);
        let (preview, preview_controls) = crate::nodes::Preview::new();
        let preview = engine.add_node("Preview", preview, 0, 2);
        connect_stereo(&mut engine, preview, out);
        let (click, metronome) = crate::nodes::Metronome::new();
        let click = engine.add_node("Metronome", click, 0, 2);
        connect_stereo(&mut engine, click, out);
        Self {
            auto_monitoring: false,
            recording: false,
            preview: preview_controls,
            reference: reference_controls,
            reference_audio: None,
            monitor,
            metronome,
            click: (false, false),
            recording_click: false,
            project_rate: engine.sample_rate(),
            engine,
            solo,
            master,
            loudness,
            tracks: HashMap::new(),
            edges: HashSet::new(),
            loop_range: None,
            tempo: None,
            plugins: Plugins {
                database: Database::default(),
                format: (48_000, 1024),
                pending_state: HashMap::new(),
                failed: HashMap::new(),
                keys: Vec::new(),
                offline: false,
                paths: None,
                files: HashMap::new(),
                retired: Vec::new(),
                feedback: Vec::new(),
                resync: false,
                low_latency: None,
                forced: Vec::new(),
            },
            overrides: HashMap::new(),
            live: LiveMidi::default(),
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    pub fn master_meter(&self) -> &MeterReadout {
        &self.master.controls.meter
    }

    pub fn track_meter(&self, id: TrackId) -> Option<&MeterReadout> {
        self.tracks.get(&id).map(|t| &t.channel.controls.meter)
    }

    pub fn loudness(&self) -> &LoudnessReadout {
        &self.loudness
    }

    pub fn loudness_mut(&mut self) -> &mut LoudnessReadout {
        &mut self.loudness
    }

    /// Converts project-rate samples to engine-rate samples.
    pub fn to_engine(&self, samples: u64) -> u64 {
        scale(samples, self.project_rate, self.engine.sample_rate())
    }

    /// Converts engine-rate samples to project-rate samples.
    pub fn to_project(&self, samples: u64) -> u64 {
        scale(samples, self.engine.sample_rate(), self.project_rate)
    }

    /// Transport position in project-rate samples.
    pub fn position(&self) -> u64 {
        self.to_project(self.engine.position())
    }

    pub fn locate(&mut self, project_samples: u64) {
        let position = self.to_engine(project_samples);
        self.engine.locate(position);
    }

    fn remove_node(&mut self, id: NodeId) {
        let _ = self.engine.remove_node(id);
        self.edges.retain(|(a, b)| a.node != id && b.node != id);
    }

    /// Brings the insert nodes of a channel in line with the model; true if nodes changed.
    #[allow(clippy::too_many_arguments)]
    fn sync_inserts(
        engine: &mut Engine,
        plugins: &mut Plugins,
        nodes: &mut ChannelNodes,
        inserts: &[InsertModel],
        removed: &mut Vec<NodeId>,
        automated: &dyn Fn(InsertId, &str) -> bool,
        live: Option<&LiveMidi>,
        low_latency: bool,
    ) -> bool {
        let mut changed = false;
        let mut old = std::mem::take(&mut nodes.inserts);
        if inserts.is_empty() {
            nodes.midi = None;
        }
        for (index, insert) in inserts.iter().enumerate() {
            let first = index == 0 && live.is_some();
            let existing = old
                .iter()
                .position(|n| {
                    n.id == insert.id
                        && n.effect == insert.effect
                        && n.first == first
                        && n.files == insert.files
                })
                .map(|i| old.remove(i));
            let mut midi = None;
            if existing.is_none()
                && first
                && let Some(live) = live
            {
                let (player, controls) = MidiPlayer::new(live.reader());
                midi = Some(player);
                nodes.midi = Some(controls);
            }
            let builtin = audiostrada_dsp::info(&insert.effect);
            let new = existing.is_none();
            let mut slot = match (existing, builtin) {
                (Some(slot), _) => slot,
                (None, Some(info)) => {
                    let Some(mut effect) = audiostrada_dsp::create(&insert.effect) else {
                        continue;
                    };
                    for (slot, path) in &insert.files {
                        let result = load_effect_file(
                            effect.as_mut(),
                            info,
                            slot,
                            &resolve(plugins.paths.as_ref(), path),
                        );
                        if let Err(e) = &result {
                            tracing::warn!("{}: {slot}: {e}", insert.effect);
                        }
                        plugins.files.insert((insert.id, slot.clone()), result);
                    }
                    effect.prepare(engine.sample_rate(), 1024);
                    let latency = effect.latency();
                    let (node, controls) =
                        Insert::new(effect, &param_values(insert, info), info.meters.len());
                    // Effects with a sidechain get two key inputs.
                    let inputs = if audiostrada_dsp::supports_sidechain(info.id) {
                        4
                    } else {
                        2
                    };
                    let node = engine.add_node(info.name, node.with_midi(midi.take()), inputs, 2);
                    let _ = engine.set_latency(node, latency);
                    InsertNodes {
                        id: insert.id,
                        effect: insert.effect.clone(),
                        node,
                        kind: InsertKind::Builtin(controls),
                        first,
                        files: insert.files.clone(),
                        latency,
                        declared: latency,
                    }
                }
                (None, None) => {
                    if plugins.failed.contains_key(&insert.id) {
                        continue;
                    }
                    match plugins.create(engine, insert, midi.take()) {
                        Ok(slot) => slot,
                        Err(e) => {
                            tracing::warn!("cannot load {}: {e}", insert.effect);
                            plugins.failed.insert(insert.id, e);
                            continue;
                        }
                    }
                }
            };
            changed |= new;
            // Low latency mode: effects slower than its limit are bypassed.
            let forced = low_latency
                && plugins
                    .low_latency
                    .is_some_and(|limit| slot.latency > limit);
            if forced {
                plugins.forced.push(insert.id);
            }
            let bypass = insert.bypass || forced;
            // A bypassed effect passes its input as it comes: no latency to compensate.
            let declared = if bypass { 0 } else { slot.latency };
            if declared != slot.declared {
                let _ = engine.set_latency(slot.node, declared);
                slot.declared = declared;
                changed = true;
            }
            match &mut slot.kind {
                InsertKind::Builtin(controls) => {
                    controls
                        .keyed
                        .store(insert.sidechain.is_some(), Ordering::Relaxed);
                    if let Some(info) = builtin {
                        let values = param_values(insert, info);
                        for ((param, value), p) in
                            controls.params.iter().zip(values).zip(info.params)
                        {
                            if !automated(insert.id, p.id) {
                                param.set(value);
                            }
                        }
                    }
                    controls.set_bypass(bypass);
                    controls.levels.gain_db.set(insert.gain_db);
                }
                InsertKind::Plugin(plugin) => {
                    for (key, &value) in &insert.params {
                        let (Ok(id), value) = (key.parse::<u32>(), f64::from(value)) else {
                            continue;
                        };
                        let known = plugin.known.get(&id).map(|&v| v as f32);
                        if known != Some(value as f32)
                            && plugin.controls.to_plugin.try_send((id, value)).is_ok()
                        {
                            plugin.known.insert(id, value);
                            plugin.instance.set_param(id, value);
                        }
                    }
                    plugin.controls.set_bypass(bypass);
                    plugin.controls.levels.gain_db.set(insert.gain_db);
                }
            }
            nodes.inserts.push(slot);
        }
        changed |= !old.is_empty();
        for slot in old {
            removed.push(plugins.retire(slot, false));
        }
        changed
    }

    /// Hears the reference track instead of the mix (when there is one).
    pub fn set_reference_listening(&mut self, on: bool) {
        self.reference.listening.set(if on { 1.0 } else { 0.0 });
    }

    pub fn reference_listening(&self) -> bool {
        self.reference.listening.get() >= 0.5
    }

    pub fn set_reference_gain(&mut self, db: f32) {
        self.reference.gain_db.set(db);
    }

    /// Gives the engine the reference audio once loaded, or takes it away.
    fn sync_reference(
        &mut self,
        project: &Project,
        paths: Option<&ProjectPaths>,
        media: &MediaPool,
    ) {
        let audio = project
            .reference
            .as_ref()
            .and_then(|r| media.audio(&resolve(paths, &r.path)))
            .cloned();
        let same = match (&audio, &self.reference_audio) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.reference.set_audio(audio.clone());
            self.reference_audio = audio;
        }
    }

    /// Brings the graph up to date with `project`; cheap when nothing changed.
    pub fn sync(&mut self, project: &Project, paths: Option<&ProjectPaths>, media: &MediaPool) {
        self.sync_reference(project, paths, media);
        self.plugins.paths = paths.cloned();
        self.plugins.forced.clear();
        let click = &project.metronome;
        self.click = (click.enabled && !self.plugins.offline, click.recording_only);
        self.apply_click();
        let m = &self.metronome;
        m.gain.set(10f32.powf(click.gain_db / 20.0));
        let sound = audiostrada_core::ClickSound::ALL
            .iter()
            .position(|s| *s == click.sound)
            .unwrap_or(0);
        m.sound.set(sound as f32);
        m.subdivision.set(click.subdivision as f32);
        m.accent.set(f32::from(u8::from(click.accent)));
        m.compound.set(f32::from(u8::from(click.compound)));
        // Groups fit the signature where the playhead is.
        let rate = project.sample_rate;
        let beat = project.tempo.sample_to_beat(self.position(), rate);
        let numerator = project.tempo.signature_at_beat(beat).numerator;
        m.groups.set(click.group_starts(numerator) as f32);
        if project.sample_rate != self.project_rate {
            self.project_rate = project.sample_rate;
            self.tracks.values_mut().for_each(|t| t.sent = None);
        }
        let rate = self.engine.sample_rate();
        let mut topology = false;
        let mut removed_nodes = Vec::new();

        let audio = |id: TrackId| {
            project
                .track(id)
                .is_some_and(|t| t.kind == TrackKind::Audio)
        };
        let removed: Vec<TrackId> = self
            .tracks
            .keys()
            .filter(|id| !audio(**id))
            .copied()
            .collect();
        for id in removed {
            if let Some(nodes) = self.tracks.remove(&id) {
                nodes.channel.controls.set_solo(false);
                removed_nodes.extend(nodes.channel.nodes());
                removed_nodes.extend([nodes.player, nodes.monitor]);
                removed_nodes.extend(nodes.sends.iter().map(|s| s.node));
                removed_nodes.extend(nodes.midi_out.iter().map(|m| m.node));
                topology = true;
            }
        }

        let audible = routing::solo_audible(project);
        let automated = automated(project);
        // Controls held by the user (or an editor's listening switch) keep the value given.
        let held: HashSet<(Channel, AutomationTarget)> = self
            .overrides
            .iter()
            .filter(|(_, flag)| flag.load(Ordering::Relaxed))
            .map(|(key, _)| key.clone())
            .collect();
        let driven =
            |key: (Channel, AutomationTarget)| automated.contains(&key) || held.contains(&key);
        let hardware = self.engine.graph().hardware_input();
        for track in project.tracks.iter().filter(|t| t.kind == TrackKind::Audio) {
            if !self.tracks.contains_key(&track.id) {
                let channel = ChannelNodes::new(&mut self.engine, &self.solo, &track.name, false);
                let (player, player_controls) = ClipPlayer::new(Vec::new());
                let player = self.engine.add_node(&track.name, player, 0, 2);
                let (monitor, monitor_gain, input_meter) = Monitor::new();
                let monitor = self.engine.add_node(&track.name, monitor, 2, 2);
                connect_stereo(&mut self.engine, player, channel.input);
                connect_stereo(&mut self.engine, monitor, channel.input);
                for ch in 0..2 {
                    self.engine
                        .connect(Port::new(hardware, 0), Port::new(monitor, ch))
                        .expect("hardware channels exist");
                }
                self.tracks.insert(
                    track.id,
                    TrackNodes {
                        channel,
                        player,
                        player_controls,
                        sent: None,
                        monitor,
                        monitor_gain,
                        input_meter,
                        monitoring: Monitoring::Auto,
                        armed: false,
                        instrument: false,
                        midi_out: None,
                        monitor_inputs: (0, 0),
                        sends: Vec::new(),
                    },
                );
                topology = true;
            }
            let inserts = effective(
                &track.inserts,
                track.fx_off || project.all_fx_off,
                &self.plugins.database,
            );
            let Some(nodes) = self.tracks.get_mut(&track.id) else {
                continue;
            };
            let (gain_db, mut mute) = routing::effective_gain(project, track.id);
            // Inside a frozen folder, the folder's render plays instead.
            let mut parent = track.parent;
            while let Some(p) = parent.and_then(|id| project.track(id)) {
                mute |= p.frozen.is_some();
                parent = p.parent;
            }
            let channel = Channel::Track(track.id);
            let is_automated = |t: AutomationTarget| automated.contains(&(channel, t));
            let controls = &nodes.channel.controls;
            if !is_automated(AutomationTarget::Gain) {
                controls.gain_db.set(gain_db);
            }
            if !is_automated(AutomationTarget::Pan) {
                controls.pan.set(track.pan);
            }
            controls.set_mute(mute);
            controls.set_solo(audible.as_ref().is_some_and(|a| a.contains(&track.id)));
            let polarity = if track.invert { -1.0 } else { 1.0 };
            nodes
                .channel
                .input_gain
                .set(polarity * db_to_gain(track.trim_db));
            nodes.armed = track.armed;
            nodes.monitoring = track.monitor;
            nodes.instrument = track
                .inserts
                .first()
                .is_some_and(|i| is_instrument(&i.effect, &self.plugins.database));

            let first = usize::from(track.input).min(HARDWARE_CHANNELS - 2);
            let inputs = (first, if track.stereo { first + 1 } else { first });
            if inputs != nodes.monitor_inputs {
                let (old, monitor) = (nodes.monitor_inputs, nodes.monitor);
                for (ch, (from, to)) in [(0, (old.0, inputs.0)), (1, (old.1, inputs.1))] {
                    self.engine
                        .disconnect(Port::new(hardware, from), Port::new(monitor, ch));
                    let _ = self
                        .engine
                        .connect(Port::new(hardware, to), Port::new(monitor, ch));
                }
                nodes.monitor_inputs = inputs;
                topology = true;
            }

            topology |= Self::sync_inserts(
                &mut self.engine,
                &mut self.plugins,
                &mut nodes.channel,
                if track.frozen.is_some() {
                    &[]
                } else {
                    &inserts
                },
                &mut removed_nodes,
                &|insert, param| {
                    driven((
                        channel,
                        AutomationTarget::Param {
                            insert,
                            param: param.to_owned(),
                        },
                    ))
                },
                Some(&self.live),
                // What is played keeps its effects (an amp, for instance).
                !(track.armed || track.monitor == Monitoring::Input),
            );
            // External instrument: the track's MIDI goes to an output port.
            match (&track.midi_out, nodes.midi_out.is_some()) {
                (Some(port), false) => {
                    let (player, controls) = MidiPlayer::new(self.live.reader());
                    let out = LiveMidi::default();
                    let node = self.engine.add_node(
                        "MIDI out",
                        crate::nodes::MidiOut::new(player, out.clone()),
                        0,
                        2,
                    );
                    nodes.midi_out = Some(MidiOutNodes {
                        node,
                        controls,
                        port: port.clone(),
                        out,
                    });
                    topology = true;
                }
                (None, true) => {
                    if let Some(m) = nodes.midi_out.take() {
                        removed_nodes.push(m.node);
                    }
                }
                (Some(port), true) => {
                    if let Some(m) = &mut nodes.midi_out {
                        m.port = port.clone();
                    }
                }
                (None, false) => {}
            }
            if nodes.channel.midi.is_some() || nodes.midi_out.is_some() {
                let ratio = f64::from(rate) / f64::from(self.project_rate.max(1));
                let clips = track
                    .clips
                    .iter()
                    .filter(|c| !c.muted)
                    .filter_map(|c| {
                        Some(MidiClip {
                            start: c.start,
                            offset: c.offset,
                            length: c.length,
                            notes: Arc::clone(c.notes.as_ref()?),
                            controls: c.controls.clone().unwrap_or_default(),
                        })
                    })
                    .collect();
                let sequence = Sequence { clips, ratio };
                if let Some(midi) = &mut nodes.channel.midi {
                    midi.armed.store(track.armed, Ordering::Relaxed);
                    midi.set(sequence.clone());
                }
                if let Some(m) = &mut nodes.midi_out {
                    m.controls.armed.store(track.armed, Ordering::Relaxed);
                    m.controls.set(sequence);
                }
            }

            // Sends, keyed by target and position (pre/post): a change of either recreates the node.
            let mut old = std::mem::take(&mut nodes.sends);
            for send in track.sends.iter().filter(|s| audio(s.target)) {
                let existing = old
                    .iter()
                    .position(|s| s.target == send.target && s.pre_fader == send.pre_fader)
                    .map(|i| old.remove(i));
                let slot = match existing {
                    Some(slot) => slot,
                    None => {
                        let follow = send.pre_fader.then(|| nodes.channel.controls.clone());
                        let (node, gain) = Gain::new(0.0, follow);
                        let node =
                            self.engine
                                .add_node(&format!("{} send", track.name), node, 2, 2);
                        topology = true;
                        SendNodes {
                            target: send.target,
                            pre_fader: send.pre_fader,
                            node,
                            gain,
                        }
                    }
                };
                if !automated.contains(&(channel, AutomationTarget::Send(send.target))) {
                    slot.gain.set(if send.mute {
                        0.0
                    } else {
                        db_to_gain(send.gain_db)
                    });
                }
                nodes.sends.push(slot);
            }
            topology |= !old.is_empty();
            removed_nodes.extend(old.into_iter().map(|s| s.node));

            let model_clips = match track.frozen {
                Some(frozen) => project
                    .sources
                    .get(&frozen.source)
                    .map(|source| audiostrada_core::Clip {
                        id: audiostrada_core::ClipId(u64::MAX),
                        source: frozen.source,
                        name: String::new(),
                        start: frozen.start,
                        offset: 0,
                        length: source.frames_at(project.sample_rate),
                        gain_db: 0.0,
                        fade_in: 0,
                        fade_out: 0,
                        fade_shape: Default::default(),
                        fade_out_shape: None,
                        muted: false,
                        stretch: 1.0,
                        pitch: 0.0,
                        controls: None,
                        notes: None,
                    })
                    .into_iter()
                    .collect(),
                None => track.clips.clone(),
            };
            let key = (model_clips, media.generation(), rate);
            if nodes.sent.as_ref() != Some(&key) {
                let clips = key
                    .0
                    .iter()
                    .filter(|c| !c.muted)
                    .filter_map(|c| {
                        let data = media.audio(&clip_media(project, paths, c)?)?;
                        Some(Clip {
                            start: scale(c.start, self.project_rate, rate),
                            offset: scale(c.offset, self.project_rate, rate),
                            length: scale(c.length, self.project_rate, rate),
                            gain: 10f32.powf(c.gain_db / 20.0),
                            fade_in: scale(c.fade_in, self.project_rate, rate),
                            fade_out: scale(c.fade_out, self.project_rate, rate),
                            fade_shape: c.fade_shape,
                            fade_out_shape: c.out_shape(),
                            data: data.clone(),
                        })
                    })
                    .collect();
                nodes.player_controls.set_clips(clips);
                nodes.sent = Some(key);
            }
        }
        if !automated.contains(&(Channel::Master, AutomationTarget::Gain)) {
            self.master.controls.gain_db.set(project.master.gain_db);
        }
        let (monitor, mix): (Vec<InsertModel>, Vec<InsertModel>) = project
            .master
            .inserts
            .iter()
            .cloned()
            .partition(|i| i.monitor);
        let monitor = if self.plugins.offline {
            Vec::new()
        } else {
            monitor
        };
        let mix = effective(
            &mix,
            project.master.fx_off || project.all_fx_off,
            &self.plugins.database,
        );
        topology |= Self::sync_inserts(
            &mut self.engine,
            &mut self.plugins,
            &mut self.monitor,
            &monitor,
            &mut removed_nodes,
            &|insert, param| {
                driven((
                    Channel::Master,
                    AutomationTarget::Param {
                        insert,
                        param: param.to_owned(),
                    },
                ))
            },
            None,
            true,
        );
        topology |= Self::sync_inserts(
            &mut self.engine,
            &mut self.plugins,
            &mut self.master,
            &mix,
            &mut removed_nodes,
            &|insert, param| {
                driven((
                    Channel::Master,
                    AutomationTarget::Param {
                        insert,
                        param: param.to_owned(),
                    },
                ))
            },
            None,
            true,
        );
        for node in removed_nodes {
            self.remove_node(node);
        }
        topology |= self.sync_edges(project);
        self.sync_automation(project, &automated);
        self.apply_monitoring();

        let loop_range = project.looping().map(|(a, b)| {
            (
                scale(a, self.project_rate, rate),
                scale(b, self.project_rate, rate),
            )
        });
        if loop_range != self.loop_range {
            self.loop_range = loop_range;
            self.engine.set_loop(loop_range);
        }
        if self.tempo.as_ref() != Some(&project.tempo) {
            self.tempo = Some(project.tempo.clone());
            self.engine.set_tempo_map(project.tempo.clone());
        }
        if topology {
            self.engine.commit();
        }
    }

    /// Routing edges the project asks for.
    fn desired_edges(&self, project: &Project) -> HashSet<(Port, Port)> {
        let mut edges = HashSet::new();
        let mut stereo = |from: NodeId, to: NodeId| {
            for ch in 0..2 {
                edges.insert((Port::new(from, ch), Port::new(to, ch)));
            }
        };
        let chain = |channel: &ChannelNodes, stereo: &mut dyn FnMut(NodeId, NodeId)| {
            let mut previous = channel.input;
            for insert in &channel.inserts {
                stereo(previous, insert.node);
                previous = insert.node;
            }
            stereo(previous, channel.strip);
        };
        chain(&self.master, &mut stereo);
        chain(&self.monitor, &mut stereo);
        // Sidechain keys: the source track's output into inputs 2 and 3 of the insert.
        let mut keys = Vec::new();
        let channels = std::iter::once((&project.master.inserts, &self.master)).chain(
            project
                .tracks
                .iter()
                .filter_map(|t| Some((&t.inserts, &self.tracks.get(&t.id)?.channel))),
        );
        for (inserts, channel) in channels {
            for insert in inserts {
                let (Some(source), Some(slot)) = (
                    insert.sidechain.and_then(|s| self.tracks.get(&s)),
                    channel.inserts.iter().find(|n| n.id == insert.id),
                ) else {
                    continue;
                };
                for ch in 0..2 {
                    keys.push((
                        Port::new(source.channel.strip, ch),
                        Port::new(slot.node, ch + 2),
                    ));
                }
            }
        }
        for track in &project.tracks {
            let Some(nodes) = self.tracks.get(&track.id) else {
                continue;
            };
            chain(&nodes.channel, &mut stereo);
            if let Some(m) = &nodes.midi_out {
                // Silent outputs: keeps the node scheduled with its track.
                stereo(m.node, nodes.channel.input);
            }
            if track.master_send {
                let target = match track.parent.and_then(|p| self.tracks.get(&p)) {
                    Some(parent) => parent.channel.input,
                    None => self.master.input,
                };
                stereo(nodes.channel.strip, target);
            }
            for send in &nodes.sends {
                let Some(target) = self.tracks.get(&send.target) else {
                    continue;
                };
                let source = if send.pre_fader {
                    nodes.channel.pre_fader()
                } else {
                    nodes.channel.strip
                };
                stereo(source, send.node);
                stereo(send.node, target.channel.input);
            }
        }
        edges.extend(keys);
        edges
    }

    /// Applies the routing edges; true if the graph changed.
    fn sync_edges(&mut self, project: &Project) -> bool {
        let desired = self.desired_edges(project);
        let stale: Vec<(Port, Port)> = self.edges.difference(&desired).copied().collect();
        let mut changed = !stale.is_empty();
        for (from, to) in stale {
            self.engine.disconnect(from, to);
            self.edges.remove(&(from, to));
        }
        for &(from, to) in desired.difference(&self.edges.clone()) {
            match self.engine.connect(from, to) {
                Ok(()) => {
                    self.edges.insert((from, to));
                    changed = true;
                }
                Err(e) => tracing::warn!("routing edge skipped: {e}"),
            }
        }
        changed
    }

    /// Starts recording; `punch` is in project samples, takes come back in engine samples.
    pub fn start_recording(
        &mut self,
        targets: Vec<TakeTarget>,
        dir: PathBuf,
        punch: Option<(u64, u64)>,
    ) -> Result<Recorder, String> {
        let punch = punch.map(|(a, b)| (self.to_engine(a), self.to_engine(b)));
        let recorder = self.engine.start_recording(targets, dir, punch)?;
        self.recording = true;
        self.apply_monitoring();
        Ok(recorder)
    }

    pub fn stop_recording(&mut self) {
        self.engine.stop_recording();
        self.recording = false;
        self.apply_monitoring();
    }

    /// Automatic input monitoring: armed tracks hear their input while stopped or recording,
    /// and play back otherwise; off, armed tracks always hear their input.
    pub fn set_auto_monitoring(&mut self, on: bool) {
        self.auto_monitoring = on;
        self.apply_monitoring();
    }

    fn apply_monitoring(&mut self) {
        let input = !self.auto_monitoring || self.recording || !self.engine.playing();
        for nodes in self.tracks.values() {
            let on = !nodes.instrument
                && match nodes.monitoring {
                    Monitoring::Auto => nodes.armed && input,
                    Monitoring::Input => true,
                    Monitoring::Off => false,
                };
            nodes.monitor_gain.set(if on { 1.0 } else { 0.0 });
        }
    }

    /// Whether a track hears its input now.
    pub fn monitoring(&self, id: TrackId) -> bool {
        self.tracks
            .get(&id)
            .is_some_and(|t| t.monitor_gain.get() >= 0.5)
    }

    /// The level of a track's input, heard or not.
    pub fn track_input_meter(&self, id: TrackId) -> Option<&MeterReadout> {
        self.tracks.get(&id).map(|t| &t.input_meter)
    }

    /// Low latency mode: effects adding more than 1 ms are bypassed, so that what is played
    /// is heard at once (their latency no longer delays the rest).
    pub fn set_low_latency(&mut self, on: bool) {
        let limit = self.engine.sample_rate() / 1000;
        self.plugins.low_latency = on.then_some(limit);
        self.plugins.resync = true;
    }

    pub fn low_latency(&self) -> bool {
        self.plugins.low_latency.is_some()
    }

    /// The inserts the low latency mode bypasses.
    pub fn low_latency_bypassed(&self) -> &[InsertId] {
        &self.plugins.forced
    }

    /// Frees what the audio thread is done with and services plugins; call regularly from the
    /// UI thread.
    pub fn update(&mut self) {
        self.engine.collect_garbage();
        self.apply_monitoring();
        self.loudness.update();
        for nodes in self.tracks.values_mut() {
            nodes.player_controls.collect();
        }
        let mut restarts = Vec::new();
        let mut crashes = Vec::new();
        let mut latency_changed = false;
        let channels = [
            (Channel::Master, &mut self.master),
            (Channel::Master, &mut self.monitor),
        ]
        .into_iter()
        .chain(
            self.tracks
                .iter_mut()
                .map(|(id, t)| (Channel::Track(*id), &mut t.channel)),
        );
        for (channel, nodes) in channels {
            for slot in &mut nodes.inserts {
                let InsertKind::Plugin(plugin) = &mut slot.kind else {
                    continue;
                };
                while let Some((id, value)) = plugin.controls.from_plugin.try_recv() {
                    plugin.known.insert(id, value);
                    plugin.instance.set_param(id, value);
                    self.plugins.feedback.push((channel, slot.id, id, value));
                }
                for request in plugin.instance.poll() {
                    match request {
                        audiostrada_plugins::PluginRequest::Restart => {
                            restarts.push((channel, slot.id))
                        }
                        audiostrada_plugins::PluginRequest::Crashed => {
                            crashes.push((channel, slot.id))
                        }
                        audiostrada_plugins::PluginRequest::ParamChanged(id, value) => {
                            if plugin.controls.to_plugin.try_send((id, value)).is_ok() {
                                plugin.known.insert(id, value);
                            }
                            self.plugins.feedback.push((channel, slot.id, id, value));
                        }
                        audiostrada_plugins::PluginRequest::Key(sym, mods) => {
                            self.plugins.keys.push((sym, mods));
                        }
                        audiostrada_plugins::PluginRequest::LatencyChanged => {
                            let latency = plugin.instance.latency();
                            if latency != plugin.latency {
                                plugin.latency = latency;
                                slot.latency = latency;
                                if !plugin.controls.bypassed() {
                                    slot.declared = latency;
                                    let _ = self.engine.set_latency(slot.node, latency);
                                    latency_changed = true;
                                }
                                // The low latency mode may now take it, or let it go.
                                self.plugins.resync |= self.plugins.low_latency.is_some();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for (channel, id) in restarts {
            self.restart_plugin(channel, id);
        }
        for (channel, id) in crashes {
            self.restart_plugin(channel, id);
            self.plugins
                .failed
                .insert(id, "the plugin crashed; right-click to reload it".into());
        }
        if latency_changed {
            self.engine.commit();
        }
        self.plugins.retired.retain_mut(|i| !i.try_deactivate());
    }

    /// The nodes holding insert `id`: for the master, its mix chain or its monitor chain.
    fn holder_mut(&mut self, channel: Channel, id: InsertId) -> Option<&mut ChannelNodes> {
        match channel {
            Channel::Master if self.master.inserts.iter().any(|n| n.id == id) => {
                Some(&mut self.master)
            }
            Channel::Master => Some(&mut self.monitor),
            Channel::Track(t) => self.tracks.get_mut(&t).map(|t| &mut t.channel),
        }
    }

    fn channel_mut(&mut self, channel: Channel) -> Option<&mut ChannelNodes> {
        match channel {
            Channel::Master => Some(&mut self.master),
            Channel::Track(id) => self.tracks.get_mut(&id).map(|t| &mut t.channel),
        }
    }

    /// Recreates a plugin instance with its current state at the next sync.
    fn restart_plugin(&mut self, channel: Channel, id: InsertId) {
        let Some(nodes) = self.holder_mut(channel, id) else {
            return;
        };
        let Some(index) = nodes.inserts.iter().position(|n| n.id == id) else {
            return;
        };
        let slot = nodes.inserts.remove(index);
        let node = self.plugins.retire(slot, true);
        self.remove_node(node);
        self.plugins.resync = true;
    }

    /// The parameter a lane drives and how its values convert.
    fn param_for(
        &mut self,
        project: &Project,
        channel: Channel,
        target: &AutomationTarget,
    ) -> Option<(SharedParam, Conversion)> {
        let vca = match channel {
            Channel::Track(id) => project
                .track(id)
                .and_then(|t| t.vca)
                .and_then(|v| project.track(v))
                .map_or(0.0, |v| v.gain_db),
            Channel::Master => 0.0,
        };
        let nodes = match target {
            AutomationTarget::Param { insert, .. } => self.holder_mut(channel, *insert)?,
            _ => self.channel_mut(channel)?,
        };
        match target {
            AutomationTarget::Gain => Some((
                nodes.controls.gain_db.clone(),
                Conversion::Db { offset: vca },
            )),
            AutomationTarget::Pan => Some((nodes.controls.pan.clone(), Conversion::Identity)),
            AutomationTarget::Mute => {
                Some((nodes.controls.mute_automation.clone(), Conversion::Identity))
            }
            AutomationTarget::Send(to) => {
                let Channel::Track(id) = channel else {
                    return None;
                };
                let send = self
                    .tracks
                    .get(&id)?
                    .sends
                    .iter()
                    .find(|s| s.target == *to)?;
                Some((send.gain.clone(), Conversion::DbToGain))
            }
            AutomationTarget::Param { insert, param } => {
                let slot = nodes.inserts.iter_mut().find(|n| n.id == *insert)?;
                match &mut slot.kind {
                    InsertKind::Builtin(controls) => {
                        let index = audiostrada_dsp::info(&slot.effect)?
                            .params
                            .iter()
                            .position(|p| p.id == param)?;
                        Some((controls.params.get(index)?.clone(), Conversion::Identity))
                    }
                    InsertKind::Plugin(plugin) => {
                        let id: u32 = param.parse().ok()?;
                        let next = plugin.automation.len();
                        let index = *plugin.automation.entry(id).or_insert(next);
                        let slot = plugin.controls.automation.get(index)?;
                        slot.id.store(id, Ordering::Relaxed);
                        slot.active.store(true, Ordering::Relaxed);
                        Some((slot.value.clone(), Conversion::Identity))
                    }
                }
            }
        }
    }

    /// Sends the automation of the project to the audio thread.
    fn sync_automation(
        &mut self,
        project: &Project,
        automated: &HashSet<(Channel, AutomationTarget)>,
    ) {
        let rate = self.engine.sample_rate();
        let ratio = f64::from(rate) / f64::from(self.project_rate.max(1));
        let mut lanes = Vec::new();
        let channels: Vec<(Channel, Vec<audiostrada_core::automation::AutomationLane>)> =
            std::iter::once((Channel::Master, project.master.automation.clone()))
                .chain(
                    project
                        .tracks
                        .iter()
                        .map(|t| (Channel::Track(t.id), t.automation.clone())),
                )
                .collect();
        for (channel, track_lanes) in channels {
            let mut mute = false;
            for lane in track_lanes {
                if !automated.contains(&(channel, lane.target.clone())) {
                    continue;
                }
                mute |= lane.target == AutomationTarget::Mute;
                let Some((target, conversion)) = self.param_for(project, channel, &lane.target)
                else {
                    continue;
                };
                let overridden = Arc::clone(
                    self.overrides
                        .entry((channel, lane.target.clone()))
                        .or_insert_with(|| Arc::new(AtomicBool::new(false))),
                );
                lanes.push(AutomationLane {
                    points: lane.points.into(),
                    target,
                    conversion,
                    overridden,
                    ratio,
                });
            }
            if !mute && let Some(nodes) = self.channel_mut(channel) {
                nodes.controls.mute_automation.set(0.0);
            }
        }
        self.overrides
            .retain(|key, flag| automated.contains(key) || flag.load(Ordering::Relaxed));
        self.engine.set_automation(AutomationSet { lanes });
    }

    /// While the user holds an automated control, it drives the parameter directly with
    /// `value`; `None` gives it back to the automation.
    pub fn set_override(
        &mut self,
        project: &Project,
        channel: Channel,
        target: &AutomationTarget,
        value: Option<f32>,
    ) {
        let flag = Arc::clone(
            self.overrides
                .entry((channel, target.clone()))
                .or_insert_with(|| Arc::new(AtomicBool::new(false))),
        );
        flag.store(value.is_some(), Ordering::Relaxed);
        // Released, an insert's parameter goes back to its own value (a lane, if any, then
        // takes over again).
        let value = value.or_else(|| static_value(project, channel, target));
        if let Some(value) = value
            && let Some((param, conversion)) = self.param_for(project, channel, target)
        {
            param.set(conversion.apply(value));
        }
    }

    /// Where MIDI input devices publish their messages.
    pub fn live_midi(&self) -> LiveMidi {
        self.live.clone()
    }

    /// Tracks sending MIDI to an output port, with the ring their MIDI goes to.
    pub fn midi_outputs(&self) -> Vec<(TrackId, String, LiveMidi)> {
        self.tracks
            .iter()
            .filter_map(|(id, n)| {
                n.midi_out
                    .as_ref()
                    .map(|m| (*id, m.port.clone(), m.out.clone()))
            })
            .collect()
    }

    /// Plays decoded audio (at the engine rate) to the outputs, or stops with `None`.
    pub fn preview(&mut self, data: Option<Arc<audiostrada_media::AudioData>>) {
        self.preview.play(data);
    }

    pub fn previewing(&self) -> bool {
        self.preview.playing()
    }

    /// Tries again to load a plugin that failed or crashed.
    pub fn reload_plugin(&mut self, id: InsertId) {
        if self.plugins.failed.remove(&id).is_some() {
            self.plugins.resync = true;
        }
    }

    /// A recording (audio or MIDI) is going on, for a click that only sounds then.
    pub fn set_recording_click(&mut self, recording: bool) {
        self.recording_click = recording;
        self.apply_click();
    }

    fn apply_click(&self) {
        let (on, recording_only) = self.click;
        let enabled = on && (!recording_only || self.recording_click);
        self.metronome.enabled.set(f32::from(u8::from(enabled)));
    }

    /// Lets plugins take longer than real time (offline rendering).
    pub fn set_offline(&mut self, offline: bool) {
        self.plugins.offline = offline;
    }

    /// Plugin database used to resolve plugin inserts.
    pub fn set_plugins(&mut self, database: Database) {
        self.plugins.database = database;
        self.plugins.failed.clear();
        self.plugins.resync = true;
    }

    pub fn plugins(&self) -> &Database {
        &self.plugins.database
    }

    /// Main-thread side of a plugin insert (parameters, window).
    pub fn plugin(&mut self, channel: Channel, id: InsertId) -> Option<&mut dyn PluginInstance> {
        let slot = self
            .holder_mut(channel, id)?
            .inserts
            .iter_mut()
            .find(|n| n.id == id)?;
        match &mut slot.kind {
            InsertKind::Plugin(plugin) => Some(plugin.instance.as_mut()),
            InsertKind::Builtin(_) => None,
        }
    }

    /// Levels before and after an insert, built-in or plugin: `[in L, in R, out L, out R,
    /// in LUFS, out LUFS, match dB]`.
    pub fn insert_levels(&mut self, channel: Channel, id: InsertId) -> Option<[f32; 7]> {
        self.level_controls(channel, id).map(|l| l.read())
    }

    /// Measures an insert's levels while its window shows them.
    pub fn set_insert_watched(&mut self, channel: Channel, id: InsertId, on: bool) {
        if let Some(levels) = self.level_controls(channel, id) {
            levels.watch(on);
        }
    }

    /// Switches an insert's Match (listening only, see [`crate::nodes::LevelControls`]).
    pub fn set_insert_match(&mut self, channel: Channel, id: InsertId, on: bool) {
        if let Some(levels) = self.level_controls(channel, id) {
            levels.set_matching(on);
        }
    }

    fn level_controls(&mut self, channel: Channel, id: InsertId) -> Option<&LevelControls> {
        let slot = self
            .holder_mut(channel, id)?
            .inserts
            .iter()
            .find(|n| n.id == id)?;
        Some(match &slot.kind {
            InsertKind::Builtin(controls) => &controls.levels,
            InsertKind::Plugin(plugin) => &plugin.controls.levels,
        })
    }

    /// Plays a note message on a track's instrument, armed or not (the editor's keyboard, the
    /// pads).
    pub fn play_key(&self, track: TrackId, data: [u8; 3]) -> bool {
        let Some(nodes) = self.tracks.get(&track) else {
            return false;
        };
        let Some(midi) = nodes.channel.midi.as_ref() else {
            return false;
        };
        // Armed, it plays as a MIDI keyboard would: heard by the armed tracks and recorded.
        if nodes.armed {
            self.live.push(data, self.engine.position());
        } else {
            midi.keys.push(data, 0);
        }
        true
    }

    /// Samples of delay an insert adds (its own, bypassed or not).
    pub fn insert_latency(&self, channel: Channel, id: InsertId) -> u32 {
        let nodes = match channel {
            Channel::Master => [&self.master, &self.monitor]
                .into_iter()
                .find_map(|c| c.inserts.iter().find(|n| n.id == id)),
            Channel::Track(t) => self
                .tracks
                .get(&t)
                .and_then(|t| t.channel.inserts.iter().find(|n| n.id == id)),
        };
        nodes.map_or(0, |n| n.latency)
    }

    /// Values reported by a built-in insert (gain reduction, spectrum...).
    pub fn insert_meters(&mut self, channel: Channel, id: InsertId) -> Option<Vec<f32>> {
        let slot = self
            .holder_mut(channel, id)?
            .inserts
            .iter()
            .find(|n| n.id == id)?;
        match &slot.kind {
            InsertKind::Builtin(controls) => {
                Some(controls.meters.iter().map(SharedParam::get).collect())
            }
            InsertKind::Plugin(_) => None,
        }
    }

    /// What a built-in effect loaded into `slot`, or why it failed.
    pub fn effect_file(&self, id: InsertId, slot: &str) -> Option<&Result<String, String>> {
        self.plugins.files.get(&(id, slot.to_owned()))
    }

    /// Why a plugin insert could not be loaded.
    /// Whether a plugin shows its own window.
    pub fn plugin_windows_open(&self) -> bool {
        std::iter::once(&self.master)
            .chain(std::iter::once(&self.monitor))
            .chain(self.tracks.values().map(|t| &t.channel))
            .flat_map(|n| &n.inserts)
            .any(|slot| matches!(&slot.kind, InsertKind::Plugin(p) if p.instance.gui_open()))
    }

    /// Closes the windows the plugins show themselves; returns those of the tracks and the
    /// master, to open again later.
    pub fn close_plugin_windows(&mut self) -> Vec<(Channel, InsertId)> {
        let channels = [
            (Some(Channel::Master), &mut self.master),
            (None, &mut self.monitor),
        ]
        .into_iter()
        .chain(
            self.tracks
                .iter_mut()
                .map(|(&t, n)| (Some(Channel::Track(t)), &mut n.channel)),
        );
        let mut closed = Vec::new();
        for (channel, nodes) in channels {
            for slot in &mut nodes.inserts {
                if let InsertKind::Plugin(p) = &mut slot.kind
                    && p.instance.gui_open()
                {
                    p.instance.close_gui();
                    closed.extend(channel.map(|c| (c, slot.id)));
                }
            }
        }
        closed
    }

    /// AudioStrada's shortcuts pressed in plugin windows since the last call.
    pub fn take_plugin_keys(&mut self) -> Vec<(u32, u16)> {
        std::mem::take(&mut self.plugins.keys)
    }

    pub fn plugin_error(&self, id: InsertId) -> Option<&str> {
        self.plugins.failed.get(&id).map(String::as_str)
    }

    /// Parameter changes made by plugins (their windows), to store in the project.
    pub fn take_param_feedback(&mut self) -> Vec<(Channel, InsertId, u32, f64)> {
        std::mem::take(&mut self.plugins.feedback)
    }

    /// True once if the graph must be synced again (plugin restarted or database changed).
    pub fn take_resync(&mut self) -> bool {
        std::mem::take(&mut self.plugins.resync)
    }

    /// Current state of every plugin insert, for saving.
    pub fn plugin_states(&mut self) -> Vec<(Channel, InsertId, Vec<u8>)> {
        let channels = [
            (Channel::Master, &mut self.master),
            (Channel::Master, &mut self.monitor),
        ]
        .into_iter()
        .chain(
            self.tracks
                .iter_mut()
                .map(|(id, t)| (Channel::Track(*id), &mut t.channel)),
        );
        let mut states = Vec::new();
        for (channel, nodes) in channels {
            for slot in &mut nodes.inserts {
                if let InsertKind::Plugin(plugin) = &mut slot.kind
                    && let Some(state) = plugin.instance.save_state()
                {
                    states.push((channel, slot.id, state));
                }
            }
        }
        states
    }

    /// Creates the processor for a stream; clips are rebuilt at the next sync and plugins are
    /// reactivated if the format changed.
    pub fn processor(&mut self, sample_rate: u32, max_frames: usize) -> EngineProcessor {
        if sample_rate != self.engine.sample_rate() {
            self.tracks.values_mut().for_each(|t| t.sent = None);
            self.loop_range = None;
        }
        let format = (sample_rate, max_frames.max(1));
        if format != self.plugins.format {
            self.plugins.format = format;
            let mut slots = Vec::new();
            let channels = [
                (Channel::Master, &mut self.master),
                (Channel::Master, &mut self.monitor),
            ]
            .into_iter()
            .chain(
                self.tracks
                    .iter_mut()
                    .map(|(id, t)| (Channel::Track(*id), &mut t.channel)),
            );
            for (channel, nodes) in channels {
                slots.extend(
                    nodes
                        .inserts
                        .iter()
                        .filter(|n| matches!(n.kind, InsertKind::Plugin(_)))
                        .map(|n| (channel, n.id)),
                );
            }
            for (channel, id) in slots {
                self.restart_plugin(channel, id);
            }
        }
        self.engine.processor(sample_rate, max_frames)
    }
}

/// The inserts as the engine runs them: all bypassed when their channel's effects are off,
/// except an instrument leading a track.
fn effective<'a>(
    inserts: &'a [InsertModel],
    off: bool,
    database: &Database,
) -> Cow<'a, [InsertModel]> {
    if !off {
        return Cow::Borrowed(inserts);
    }
    inserts
        .iter()
        .enumerate()
        .map(|(n, i)| {
            let mut i = i.clone();
            i.bypass |= !(n == 0 && is_instrument(&i.effect, database));
            i
        })
        .collect()
}

/// The value an insert parameter has in the project, its default if never set.
fn static_value(project: &Project, channel: Channel, target: &AutomationTarget) -> Option<f32> {
    let AutomationTarget::Param { insert, param } = target else {
        return None;
    };
    let insert = routing::inserts(project, channel)
        .iter()
        .find(|i| i.id == *insert)?;
    insert.params.get(param).copied().or_else(|| {
        audiostrada_dsp::info(&insert.effect)?
            .params
            .iter()
            .find(|p| p.id == param)
            .map(|p| p.default)
    })
}

/// A built-in or plugin instrument, which makes sound from MIDI notes.
fn is_instrument(effect: &str, database: &Database) -> bool {
    audiostrada_dsp::info(effect).is_some_and(|d| d.instrument)
        || database.find(effect).is_some_and(|d| d.instrument)
}

fn scale(samples: u64, from: u32, to: u32) -> u64 {
    if from == to || from == 0 {
        samples
    } else {
        (samples as f64 * f64::from(to) / f64::from(from)).round() as u64
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use audiostrada_core::{History, Source};
    use audiostrada_io::ChannelBuffers;
    use audiostrada_media::{AudioData, write_wav};

    use super::*;

    fn render(processor: &mut EngineProcessor, blocks: usize) -> Vec<f32> {
        let mut storage = ChannelBuffers::new(0, 2, 64);
        let mut out = Vec::new();
        for _ in 0..blocks {
            processor.render(&mut storage.buffers(64));
            out.extend_from_slice(&storage.output(0)[..64]);
        }
        out
    }

    struct Fixture {
        dir: PathBuf,
        file: PathBuf,
        history: History,
        media: MediaPool,
        player: ProjectPlayer,
    }

    fn fixture(name: &str, samples: Vec<f32>) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("audiostrada-project-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.wav");
        let frames = samples.len() as u64;
        write_wav(&file, &AudioData::new(vec![samples], 48_000)).unwrap();
        let mut history = History::new(Project::new("Test", 48_000));
        history.edit("Import", |p| {
            let s = p.add_source(Source {
                path: file.clone(),
                channels: 1,
                sample_rate: 48_000,
                frames,
            });
            let t = p.add_track("a");
            p.add_clip(t, s, 0);
        });
        Fixture {
            dir,
            file,
            history,
            media: MediaPool::new(48_000, None),
            player: ProjectPlayer::new(EngineConfig { workers: Some(0) }),
        }
    }

    #[test]
    fn plays_the_project_and_follows_edits() {
        let samples: Vec<f32> = (0..2000)
            .map(|i| ((i * 13) % 97) as f32 / 97.0 - 0.5)
            .collect();
        let mut f = fixture("edits", samples.clone());
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        let out = render(&mut processor, 10);
        assert_eq!(&out[..640], &samples[..640], "bit-exact at unity");

        let t = f.history.project().tracks[0].id;
        f.history
            .edit("Mute", |p| p.track_mut(t).unwrap().mute = true);
        f.player.sync(f.history.project(), None, &f.media);
        render(&mut processor, 1);
        assert!(render(&mut processor, 2).iter().all(|&s| s == 0.0), "muted");

        f.history.undo();
        f.history.edit("Remove", |p| {
            p.remove_track(t);
        });
        f.player.sync(f.history.project(), None, &f.media);
        render(&mut processor, 1);
        f.player.update();
        assert!(
            render(&mut processor, 2).iter().all(|&s| s == 0.0),
            "removed"
        );
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn sidechain_ducks_a_track_from_another() {
        let dir =
            std::env::temp_dir().join(format!("audiostrada-sidechain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let level = |kick: f32| {
            let bass = dir.join("bass.wav");
            let key = dir.join(format!("kick-{kick}.wav"));
            write_wav(&bass, &AudioData::new(vec![vec![0.5; 48_000]], 48_000)).unwrap();
            write_wav(&key, &AudioData::new(vec![vec![kick; 48_000]], 48_000)).unwrap();
            let mut p = Project::new("Test", 48_000);
            let source = |path: &std::path::Path| Source {
                path: path.to_path_buf(),
                channels: 1,
                sample_rate: 48_000,
                frames: 48_000,
            };
            let (sb, sk) = (p.add_source(source(&bass)), p.add_source(source(&key)));
            let (b, k) = (p.add_track("Bass"), p.add_track("Kick"));
            p.add_clip(b, sb, 0);
            p.add_clip(k, sk, 0);
            p.track_mut(k).unwrap().master_send = false;
            let mut params = std::collections::BTreeMap::new();
            params.insert("threshold".to_owned(), -30.0);
            params.insert("ratio".to_owned(), 20.0);
            params.insert("attack".to_owned(), 0.1);
            let c = routing::add_insert(&mut p, Channel::Track(b), "compressor", params).unwrap();
            p.track_mut(b).unwrap().inserts[0].sidechain = Some(k);
            let _ = c;
            let mut media = MediaPool::new(48_000, None);
            media.retain(&media_paths(&p, None));
            assert!(media.wait(Duration::from_secs(10)));
            let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
            let mut processor = player.processor(48_000, 480);
            player.sync(&p, None, &media);
            player.engine_mut().play();
            let out = render(&mut processor, 40);
            out[out.len() - 480..]
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
        };
        let quiet = level(0.0);
        let ducked = level(0.9);
        assert!((quiet - 0.5).abs() < 0.02, "no key: untouched ({quiet})");
        assert!(ducked < 0.2, "kick ducks the bass ({ducked})");
    }

    #[test]
    fn monitor_inserts_are_heard_but_not_rendered() {
        let samples = vec![0.5f32; 48_000];
        let mut f = fixture("monitor", samples);
        let mut params = std::collections::BTreeMap::new();
        params.insert("gain".to_owned(), -20.0);
        f.history.edit("Monitor", |p| {
            routing::add_insert(p, Channel::Master, "utility", params);
            p.master.inserts[0].monitor = true;
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let level = |player: &mut ProjectPlayer| {
            let mut processor = player.processor(48_000, 64);
            player.sync(f.history.project(), None, &f.media);
            player.engine_mut().play();
            let out = render(&mut processor, 20);
            out[out.len() - 64..]
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
        };
        let live = level(&mut f.player);
        let mut offline = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        offline.set_offline(true);
        let rendered = level(&mut offline);
        assert!(
            (live - 0.05).abs() < 0.005,
            "heard through the monitor chain: {live}"
        );
        assert!(
            (rendered - 0.5).abs() < 0.005,
            "left out of exports: {rendered}"
        );
        f.player.update();
        let peak = f.player.loudness().peak();
        assert!(
            peak > 0.4,
            "meters read the mix before the monitor chain: {peak}"
        );
    }

    #[test]
    fn metronome_clicks_only_live() {
        let mut project = Project::new("Test", 48_000);
        project.metronome.enabled = true;
        let media = MediaPool::new(48_000, None);
        let energy = |player: &mut ProjectPlayer| {
            let mut processor = player.processor(48_000, 480);
            let mut storage = ChannelBuffers::new(0, 2, 480);
            player.sync(&project, None, &media);
            player.engine_mut().play();
            let mut sum = 0.0;
            for _ in 0..20 {
                processor.render(&mut storage.buffers(480));
                sum += storage.output(0).iter().map(|s| s * s).sum::<f32>();
            }
            sum
        };
        let mut live = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        assert!(energy(&mut live) > 1.0, "click on beat 1");
        let mut offline = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        offline.set_offline(true);
        assert_eq!(energy(&mut offline), 0.0, "never in exports");
    }

    #[test]
    fn armed_tracks_monitor_their_input() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let media = MediaPool::new(48_000, None);
        let mut processor = player.processor(48_000, 64);
        let mut storage = ChannelBuffers::new(4, 2, 64);
        storage.input_mut(2).fill(0.5);
        let mut render = |player: &mut ProjectPlayer, history: &History| {
            player.sync(history.project(), None, &media);
            processor.render(&mut storage.buffers(64));
            processor.render(&mut storage.buffers(64));
            storage.output(0)[63]
        };
        history.edit("Track", |p| {
            let t = p.add_track("Vox");
            p.track_mut(t).unwrap().input = 2;
        });
        assert_eq!(render(&mut player, &history), 0.0, "not armed");
        let t = history.project().tracks[0].id;
        history.edit("Arm", |p| p.track_mut(t).unwrap().armed = true);
        assert!((render(&mut player, &history) - 0.5).abs() < 1e-3);
        history.edit("Input", |p| p.track_mut(t).unwrap().input = 0);
        assert_eq!(render(&mut player, &history), 0.0, "input 1 is silent");
        // Led by an instrument, the track is armed for MIDI and never hears the inputs.
        history.edit("Synth", |p| {
            p.track_mut(t).unwrap().input = 2;
            routing::add_insert(p, Channel::Track(t), "synth", Default::default());
        });
        assert_eq!(render(&mut player, &history), 0.0, "instrument track");
    }

    #[test]
    fn automatic_monitoring_follows_the_transport() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let media = MediaPool::new(48_000, None);
        let mut processor = player.processor(48_000, 64);
        let mut storage = ChannelBuffers::new(4, 2, 64);
        storage.input_mut(0).fill(0.5);
        history.edit("Track", |p| {
            let t = p.add_track("Vox");
            p.track_mut(t).unwrap().armed = true;
        });
        player.sync(history.project(), None, &media);
        player.set_auto_monitoring(true);
        // The UI thread follows the transport once the audio thread has applied it.
        let mut level = |player: &mut ProjectPlayer| {
            processor.render(&mut storage.buffers(64));
            player.update();
            processor.render(&mut storage.buffers(64));
            processor.render(&mut storage.buffers(64));
            storage.output(0)[63]
        };
        assert!(level(&mut player) > 0.4, "stopped: input heard");
        player.engine_mut().play();
        assert_eq!(level(&mut player), 0.0, "playing: the track plays back");
        player.set_auto_monitoring(false);
        assert!(
            level(&mut player) > 0.4,
            "always: input heard while playing"
        );
    }

    #[test]
    fn armed_instruments_do_not_hear_the_audio_input() {
        for effect in ["synth", "drums"] {
            let mut history = History::new(Project::new("Test", 48_000));
            let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
            let media = MediaPool::new(48_000, None);
            let mut processor = player.processor(48_000, 64);
            let mut storage = ChannelBuffers::new(4, 2, 64);
            storage.input_mut(0).fill(0.5);
            history.edit("Track", |p| {
                let t = p.add_track("Keys");
                routing::add_insert(p, Channel::Track(t), effect, Default::default());
                p.track_mut(t).unwrap().armed = true;
            });
            player.sync(history.project(), None, &media);
            player.update();
            let mut peak = 0.0f32;
            for _ in 0..20 {
                processor.render(&mut storage.buffers(64));
                player.update();
                peak = peak.max(storage.output(0).iter().fold(0.0, |m, s| m.max(s.abs())));
            }
            assert!(peak < 1e-4, "{effect}: the input is heard ({peak})");
        }
    }

    #[test]
    fn monitoring_follows_each_tracks_mode() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let media = MediaPool::new(48_000, None);
        let mut processor = player.processor(48_000, 64);
        let mut storage = ChannelBuffers::new(4, 2, 64);
        storage.input_mut(0).fill(0.5);
        let mut id = TrackId(0);
        history.edit("Track", |p| id = p.add_track("Gtr"));
        let mut level = |player: &mut ProjectPlayer, history: &History| {
            player.sync(history.project(), None, &media);
            player.update();
            for _ in 0..3 {
                processor.render(&mut storage.buffers(64));
            }
            storage.output(0)[63]
        };
        assert_eq!(level(&mut player, &history), 0.0, "auto, not armed: silent");
        let input = player.track_input_meter(id).unwrap().take_peak(0);
        assert!(input > 0.4, "its input is measured all the same");
        history.edit("Input", |p| {
            p.track_mut(id).unwrap().monitor = Monitoring::Input
        });
        assert!(level(&mut player, &history) > 0.4, "input: heard unarmed");
        assert!(player.monitoring(id));
        history.edit("Off", |p| {
            let t = p.track_mut(id).unwrap();
            t.monitor = Monitoring::Off;
            t.armed = true;
        });
        assert_eq!(level(&mut player, &history), 0.0, "off: silent, even armed");
    }

    #[test]
    fn low_latency_bypasses_slow_effects_except_on_played_tracks() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let media = MediaPool::new(48_000, None);
        let _processor = player.processor(48_000, 64);
        let mut inserts = Vec::new();
        history.edit("Tracks", |p| {
            let vox = p.add_track("Vox");
            p.track_mut(vox).unwrap().armed = true;
            let bed = p.add_track("Bed");
            for t in [vox, bed] {
                inserts.push(
                    routing::add_insert(p, Channel::Track(t), "limiter", Default::default())
                        .unwrap(),
                );
            }
        });
        player.sync(history.project(), None, &media);
        assert!(player.engine().shared().latency() > 0);
        player.set_low_latency(true);
        assert!(player.take_resync());
        player.sync(history.project(), None, &media);
        assert_eq!(player.low_latency_bypassed(), [inserts[1]]);
        assert!(
            player.engine().shared().latency() > 0,
            "the armed track keeps its limiter"
        );
        let vox = history.project().tracks[0].id;
        history.edit("Disarm", |p| p.track_mut(vox).unwrap().armed = false);
        player.sync(history.project(), None, &media);
        assert_eq!(player.low_latency_bypassed().len(), 2);
        assert_eq!(player.engine().shared().latency(), 0);
        player.set_low_latency(false);
        player.sync(history.project(), None, &media);
        assert!(player.low_latency_bypassed().is_empty());
        assert!(player.engine().shared().latency() > 0);
    }

    /// Renders a constant 0.5 clip through `edit` and returns the last left sample.
    fn level(name: &str, edit: impl FnOnce(&mut Project, TrackId)) -> f32 {
        let mut f = fixture(name, vec![0.5; 48_000]);
        let t = f.history.project().tracks[0].id;
        f.history.edit("Routing", |p| edit(p, t));
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        let out = render(&mut processor, 8);
        std::fs::remove_dir_all(&f.dir).unwrap();
        out[out.len() - 1]
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn buses_sends_and_folders() {
        assert!(close(level("plain", |_, _| {}), 0.5));
        // Track into a folder bus at -6 dB.
        let folder = level("folder", |p, t| {
            let bus = p.add_track("bus");
            p.track_mut(bus).unwrap().gain_db = -6.0206;
            routing::set_parent(p, t, Some(bus)).unwrap();
        });
        assert!(close(folder, 0.25), "{folder}");
        // Output off, post-fader send to a bus: only the send is heard.
        let send = level("send", |p, t| {
            let bus = p.add_track("bus");
            routing::add_send(p, t, bus).unwrap();
            p.track_mut(t).unwrap().master_send = false;
            p.track_mut(t).unwrap().sends[0].gain_db = -6.0206;
        });
        assert!(close(send, 0.25), "{send}");
        // Pre-fader send ignores the fader; muting the track silences it.
        let pre = level("pre", |p, t| {
            let bus = p.add_track("bus");
            routing::add_send(p, t, bus).unwrap();
            let track = p.track_mut(t).unwrap();
            track.master_send = false;
            track.gain_db = -60.0;
            track.sends[0].pre_fader = true;
        });
        assert!(close(pre, 0.5), "{pre}");
        let muted = level("pre-muted", |p, t| {
            let bus = p.add_track("bus");
            routing::add_send(p, t, bus).unwrap();
            let track = p.track_mut(t).unwrap();
            track.master_send = false;
            track.mute = true;
            track.sends[0].pre_fader = true;
        });
        assert_eq!(muted, 0.0);
    }

    #[test]
    fn inserts_trim_polarity_vca_and_solo() {
        let insert = level("insert", |p, t| {
            let id =
                routing::add_insert(p, routing::Channel::Track(t), "utility", Default::default())
                    .unwrap();
            routing::set_insert_param(p, routing::Channel::Track(t), id, "gain", -6.0206);
            routing::add_insert(p, routing::Channel::Master, "utility", Default::default());
        });
        assert!(close(insert, 0.25), "{insert}");
        let bypassed = level("bypass", |p, t| {
            let id =
                routing::add_insert(p, routing::Channel::Track(t), "utility", Default::default())
                    .unwrap();
            routing::set_insert_param(p, routing::Channel::Track(t), id, "gain", -6.0206);
            routing::set_bypass(p, routing::Channel::Track(t), id, true);
        });
        assert!(close(bypassed, 0.5), "{bypassed}");
        let all_off = level("fx-off", |p, t| {
            let id =
                routing::add_insert(p, routing::Channel::Track(t), "utility", Default::default())
                    .unwrap();
            routing::set_insert_param(p, routing::Channel::Track(t), id, "gain", -6.0206);
            p.all_fx_off = true;
        });
        assert!(close(all_off, 0.5), "{all_off}");
        let trim = level("trim", |p, t| {
            let track = p.track_mut(t).unwrap();
            track.trim_db = -6.0206;
            track.invert = true;
        });
        assert!(close(trim, -0.25), "{trim}");
        let vca = level("vca", |p, t| {
            let v = routing::add_vca(p, "vca");
            p.track_mut(v).unwrap().gain_db = -6.0206;
            routing::set_vca(p, t, Some(v)).unwrap();
        });
        assert!(close(vca, 0.25), "{vca}");
        let soloed_other = level("solo", |p, _| {
            let other = p.add_track("other");
            p.track_mut(other).unwrap().solo = true;
        });
        assert_eq!(soloed_other, 0.0);
    }

    #[test]
    fn effect_meters_reach_the_ui() {
        let samples: Vec<f32> = (0..48_000)
            .map(|i| 0.25 * (i as f32 * 997.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        let mut f = fixture("meters", samples);
        let t = f.history.project().tracks[0].id;
        let mut id = None;
        f.history.edit("Insert", |p| {
            id = routing::add_insert(p, Channel::Track(t), "maximizer", Default::default());
        });
        let id = id.unwrap();
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        let rest = f.player.insert_meters(Channel::Track(t), id).unwrap();
        assert!(rest[2] < -100.0, "silent before playing: {rest:?}");
        f.player.engine_mut().play();
        render(&mut processor, 100);
        let meters = f.player.insert_meters(Channel::Track(t), id).unwrap();
        assert!((meters[2] + 12.0).abs() < 0.5, "input peak: {meters:?}");
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn insert_levels_and_output_gain() {
        let samples: Vec<f32> = (0..96_000)
            .map(|i| 0.5 * (i as f32 * 997.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        let mut f = fixture("insert-levels", samples);
        let t = f.history.project().tracks[0].id;
        let mut id = None;
        f.history.edit("Insert", |p| {
            id = routing::add_insert(p, Channel::Track(t), "utility", Default::default());
            routing::set_insert_param(p, Channel::Track(t), id.unwrap(), "gain", -6.0);
        });
        let id = id.unwrap();
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        f.player.set_insert_watched(Channel::Track(t), id, true);
        render(&mut processor, 750);
        let [in_l, _, out_l, _, in_lufs, out_lufs, _] =
            f.player.insert_levels(Channel::Track(t), id).unwrap();
        assert!((in_l - out_l - 6.0).abs() < 0.2, "{in_l} {out_l}");
        assert!(
            (in_lufs - out_lufs - 6.0).abs() < 0.2,
            "{in_lufs} {out_lufs}"
        );
        // Match: what is heard comes back to the input's level, the insert is unchanged.
        f.player.set_insert_match(Channel::Track(t), id, true);
        render(&mut processor, 50);
        let heard = render(&mut processor, 50)
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        let matched = f.player.insert_levels(Channel::Track(t), id).unwrap()[6];
        assert!((matched - 6.0).abs() < 0.2, "{matched}");
        assert!((heard - 0.5).abs() < 0.02, "{heard}");
        f.player.set_insert_match(Channel::Track(t), id, false);
        render(&mut processor, 50);
        let heard = render(&mut processor, 50)
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((heard - 0.25).abs() < 0.02, "off again: {heard}");
        f.history.edit("Gain", |p| {
            routing::set_insert_gain(p, Channel::Track(t), id, 6.0)
        });
        f.player.sync(f.history.project(), None, &f.media);
        render(&mut processor, 100);
        let [in_l, _, out_l, ..] = f.player.insert_levels(Channel::Track(t), id).unwrap();
        assert!((in_l - out_l).abs() < 0.2, "matched: {in_l} {out_l}");
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn the_reference_is_heard_instead_of_the_mix() {
        let mut f = fixture("reference", vec![0.25; 48_000]);
        let reference = f.dir.join("reference.wav");
        write_wav(&reference, &AudioData::new(vec![vec![0.8; 48_000]], 48_000)).unwrap();
        f.history.edit("Reference", |p| {
            p.reference = Some(audiostrada_core::ReferenceTrack {
                path: reference.clone(),
                matched: true,
            });
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        let last = |out: Vec<f32>| *out.last().unwrap();
        assert!(
            (last(render(&mut processor, 10)) - 0.25).abs() < 1e-4,
            "the mix"
        );
        f.player.set_reference_listening(true);
        f.player.set_reference_gain(-6.0206);
        render(&mut processor, 4);
        let heard = last(render(&mut processor, 4));
        assert!(
            (heard - 0.4).abs() < 1e-3,
            "the reference, 6 dB down: {heard}"
        );
        f.player.set_reference_listening(false);
        render(&mut processor, 4);
        assert!(
            (last(render(&mut processor, 4)) - 0.25).abs() < 1e-4,
            "the mix again"
        );
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn plugin_inserts_when_available() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_CLAP") else {
            eprintln!("AUDIOSTRADA_TEST_CLAP not set, skipping");
            return;
        };
        let mut database = Database::default();
        database.refresh(&[PathBuf::from(dir)], audiostrada_plugins::scan_file);
        let plugin = database
            .plugins()
            .find(|p| !p.instrument && p.name.contains("Reverb"))
            .cloned()
            .expect("AUDIOSTRADA_TEST_CLAP holds a reverb (Dragonfly)");
        let mut f = fixture(
            "plugin",
            (0..48_000)
                .map(|i| if i % 4800 == 0 { 1.0 } else { 0.0 })
                .collect(),
        );
        let t = f.history.project().tracks[0].id;
        f.history.edit("Insert", |p| {
            routing::add_insert(p, Channel::Track(t), &plugin.key(), Default::default());
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        f.player.set_plugins(database);
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        let out = render(&mut processor, 300);
        f.player.update();
        let tail: f32 = out[100..4800].iter().map(|s| s.abs()).sum();
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(tail > 0.0, "the reverb adds a tail after each click");
        let id = f.history.project().tracks[0].inserts[0].id;
        assert!(f.player.plugin(Channel::Track(t), id).is_some());
        assert_eq!(f.player.plugin_states().len(), 1);
        f.history.edit("Remove", |p| {
            routing::remove_insert(p, Channel::Track(t), id)
        });
        f.player.sync(f.history.project(), None, &f.media);
        render(&mut processor, 2);
        f.player.update();
        drop(processor);
        f.player.update();
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    /// Exports a MIDI clip played by the instrument in AUDIOSTRADA_TEST_LV2_INSTRUMENT.
    #[test]
    fn plugin_instruments_play_clips_when_available() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_LV2_INSTRUMENT") else {
            eprintln!("AUDIOSTRADA_TEST_LV2_INSTRUMENT not set, skipping");
            return;
        };
        let mut database = Database::default();
        database.refresh(&[PathBuf::from(dir)], audiostrada_plugins::scan_file);
        let plugin = database
            .plugins()
            .find(|p| p.instrument)
            .cloned()
            .expect("an instrument");
        let mut history = History::new(Project::new("Test", 48_000));
        history.edit("Piano", |p| {
            let t = p.add_track("Piano");
            routing::add_insert(p, Channel::Track(t), &plugin.key(), Default::default());
            let notes = (0..4)
                .map(|i| audiostrada_core::Note {
                    start: i * 12_000,
                    length: 10_000,
                    key: 60 + i as u8 * 4,
                    velocity: 100,
                    channel: 0,
                    ..Default::default()
                })
                .collect();
            p.add_midi_clip(t, 0, 48_000, notes);
        });
        let media = MediaPool::new(48_000, None);
        let mut render = crate::export::OfflineRender::new(
            history.project(),
            None,
            &media,
            database,
            0,
            48_000,
            0.5,
        );
        while !render.render(4096) {}
        let audio = render.finish().audio;
        let energy: f32 = (0..audio.channels())
            .flat_map(|c| audio.channel(c))
            .map(|s| s * s)
            .sum();
        eprintln!("{}: exported energy {energy}", plugin.name);
        assert!(energy > 0.01, "{} is silent in exports", plugin.name);
    }

    #[test]
    fn a_released_override_gives_the_parameter_its_value_back() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut insert = None;
        history.edit("EQ", |p| {
            let t = p.add_track("A");
            insert = routing::add_insert(p, Channel::Track(t), "eq", Default::default());
        });
        let (t, insert) = (history.project().tracks[0].id, insert.unwrap());
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let _processor = player.processor(48_000, 64);
        player.sync(history.project(), None, &MediaPool::new(48_000, None));
        let target = AutomationTarget::Param {
            insert,
            param: "solo".into(),
        };
        let channel = Channel::Track(t);
        let project = history.project().clone();
        let value = |player: &mut ProjectPlayer| {
            player
                .param_for(&project, channel, &target)
                .unwrap()
                .0
                .get()
        };
        player.set_override(&project, channel, &target, Some(3.0));
        assert_eq!(value(&mut player), 3.0);
        // Turning another knob meanwhile keeps it.
        history.edit("Gain", |p| {
            routing::set_insert_param(p, channel, insert, "output", -3.0)
        });
        let media = MediaPool::new(48_000, None);
        player.sync(history.project(), None, &media);
        player.sync(history.project(), None, &media);
        assert_eq!(value(&mut player), 3.0, "held while the project changes");
        player.set_override(&project, channel, &target, None);
        assert_eq!(value(&mut player), 0.0, "back to the project's value");
    }

    #[test]
    fn automation_drives_controls() {
        use audiostrada_core::automation::{AutomationMode, CurveShape, lane_mut};
        let gain = level("auto-gain", |p, t| {
            let track = p.track_mut(t).unwrap();
            track.gain_db = 6.0;
            lane_mut(track, &AutomationTarget::Gain).add(0, -6.0206, CurveShape::Linear);
        });
        assert!(close(gain, 0.25), "{gain}");
        let off = level("auto-off", |p, t| {
            let track = p.track_mut(t).unwrap();
            track.automation_mode = AutomationMode::Off;
            lane_mut(track, &AutomationTarget::Gain).add(0, -6.0206, CurveShape::Linear);
        });
        assert!(close(off, 0.5), "{off}");
        let muted = level("auto-mute", |p, t| {
            lane_mut(p.track_mut(t).unwrap(), &AutomationTarget::Mute).add(
                0,
                1.0,
                CurveShape::Step,
            );
        });
        assert_eq!(muted, 0.0);
        let utility = level("auto-param", |p, t| {
            let id =
                routing::add_insert(p, Channel::Track(t), "utility", Default::default()).unwrap();
            let target = AutomationTarget::Param {
                insert: id,
                param: "gain".into(),
            };
            lane_mut(p.track_mut(t).unwrap(), &target).add(0, -6.0206, CurveShape::Linear);
        });
        assert!(close(utility, 0.25), "{utility}");

        // Holding the control overrides the lane.
        let mut f = fixture("auto-override", vec![0.5; 48_000]);
        let t = f.history.project().tracks[0].id;
        f.history.edit("Lane", |p| {
            lane_mut(p.track_mut(t).unwrap(), &AutomationTarget::Gain).add(
                0,
                -60.0,
                CurveShape::Linear,
            );
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        assert!(render(&mut processor, 4)[255].abs() < 0.001);
        let project = f.history.project().clone();
        f.player.set_override(
            &project,
            Channel::Track(t),
            &AutomationTarget::Gain,
            Some(0.0),
        );
        let out = render(&mut processor, 4);
        assert!(close(out[255], 0.5), "{}", out[255]);
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn midi_clips_play_the_track_instrument() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let media = MediaPool::new(48_000, None);
        let mut processor = player.processor(48_000, 64);
        history.edit("Synth", |p| {
            let t = p.add_track("Keys");
            routing::add_insert(p, Channel::Track(t), "synth", Default::default());
            let note = audiostrada_core::Note {
                start: 4800,
                length: 9600,
                key: 60,
                velocity: 110,
                channel: 0,
                ..Default::default()
            };
            p.add_midi_clip(t, 0, 48_000, vec![note]);
        });
        player.sync(history.project(), None, &media);
        player.engine_mut().play();
        let out = render(&mut processor, 200);
        assert!(
            out[..4800].iter().all(|&s| s == 0.0),
            "silent before the note"
        );
        assert!(
            out[5000..].iter().any(|s| s.abs() > 0.01),
            "the synth plays the note"
        );
    }

    #[test]
    fn midi_output_sends_the_clip_notes() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let media = MediaPool::new(48_000, None);
        let mut processor = player.processor(48_000, 64);
        history.edit("External", |p| {
            let t = p.add_track("Hardware synth");
            p.track_mut(t).unwrap().midi_out = Some("Port".into());
            let note = audiostrada_core::Note {
                start: 4800,
                length: 4800,
                key: 64,
                velocity: 90,
                channel: 2,
                ..Default::default()
            };
            p.add_midi_clip(t, 0, 48_000, vec![note]);
        });
        player.sync(history.project(), None, &media);
        let outputs = player.midi_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].1, "Port");
        let mut reader = outputs[0].2.reader();
        player.engine_mut().play();
        render(&mut processor, 200);
        let mut sent = Vec::new();
        reader.drain(|data, _| sent.push(data));
        assert!(sent.contains(&[0x92, 64, 90]), "note on: {sent:?}");
        assert!(
            sent.iter()
                .any(|d| d[0] & 0xF0 == 0x80 || (d[0] & 0xF0 == 0x90 && d[2] == 0)),
            "note off: {sent:?}"
        );
        history.edit("Internal", |p| {
            let t = p.tracks[0].id;
            p.track_mut(t).unwrap().midi_out = None;
        });
        player.sync(history.project(), None, &media);
        assert!(player.midi_outputs().is_empty());
    }

    #[test]
    fn timeline_scales_to_the_engine_rate() {
        let mut f = fixture("scale", vec![0.25; 4800]);
        let t = f.history.project().tracks[0].id;
        f.history
            .edit("Move", |p| p.track_mut(t).unwrap().clips[0].start = 4800);
        f.media.set_rate(96_000);
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(96_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        let out = render(&mut processor, 300);
        // 4800 samples at 48 kHz start at 9600 samples at 96 kHz.
        assert!(out[..9590].iter().all(|&s| s == 0.0));
        assert!(out[9700..19000].iter().all(|&s| (s - 0.25).abs() < 0.02));
        assert_eq!(f.player.to_project(9600), 4800);
        let _ = Arc::clone(f.media.audio(&f.file).unwrap());
        std::fs::remove_dir_all(&f.dir).unwrap();
    }
}
