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
    Clip, ClipPlayer, ConsoleControls, Gain, InputStage, Insert, InsertControls, LevelControls,
    MidiClip, MidiControls, MidiLayer, MidiPlayer, MidiShare, Monitor, PlayerControls,
    PluginControls, PluginNode, RecordWindow, Sequence, SoloBus, Strip, StripControls,
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
    /// Values its window set, for its audio side, waiting for room in the queue: a preset
    /// chosen in a VST3 window comes as an edit of each of its parameters, thousands at once.
    unsent: Vec<(u32, f64)>,
    latency: u32,
}

impl PluginSlot {
    /// Keeps a value for the plugin's audio side: the last one of each parameter.
    fn hold(&mut self, id: u32, value: f64) {
        match self.unsent.iter_mut().find(|(i, _)| *i == id) {
            Some(held) => held.1 = value,
            None => self.unsent.push((id, value)),
        }
    }

    /// Sends the values held, in order, as many as the queue takes; the others wait.
    fn flush(&mut self) {
        let mut sent = 0;
        for &(id, value) in &self.unsent {
            if self.controls.to_plugin.try_send((id, value)).is_err() {
                break;
            }
            self.known.insert(id, value);
            sent += 1;
        }
        self.unsent.drain(..sent);
    }
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
    /// An instrument after the first, layered on it: it plays the same notes.
    layer: bool,
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
    /// Instances deactivated and activated again in place at their request (a CLAP restart,
    /// a VST3 latency or bus change, which a preset can bring): their window stays open and
    /// their state as it is. They wait for their audio side to be dropped first.
    restarting: Vec<Restarting>,
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

/// A plugin instance between its deactivation and its activation again (see
/// `Plugins::restarting`).
struct Restarting {
    id: InsertId,
    instance: Box<dyn PluginInstance>,
    /// The values known to be the plugin's: not sent to it again.
    known: HashMap<u32, f64>,
    /// Values for its audio side not sent yet: they go to the new one.
    unsent: Vec<(u32, f64)>,
    /// Still waiting at the last sync (its insert is still there).
    seen: bool,
}

impl std::fmt::Debug for Restarting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Restarting")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
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
        layer: Option<MidiLayer>,
    ) -> Result<InsertNodes, String> {
        let descriptor = self
            .database
            .find(&insert.effect)
            .cloned()
            .ok_or_else(|| format!("plugin {} is not installed", insert.effect))?;
        // The same plugin restarting in place (already deactivated, see `restarting`).
        let restarted = self
            .restarting
            .iter()
            .position(|r| r.id == insert.id && r.instance.descriptor().id == descriptor.id)
            .map(|i| self.restarting.swap_remove(i));
        // Until activated: a plugin that crashes AudioStrada meanwhile is refused next time.
        let _loading = restarted
            .is_none()
            .then(|| audiostrada_plugins::guard::loading(&descriptor));
        let (mut instance, known, unsent) = match restarted {
            Some(r) => (r.instance, r.known, r.unsent),
            None => {
                let mut instance = audiostrada_plugins::instantiate(&descriptor)?;
                let state = self
                    .pending_state
                    .remove(&insert.id)
                    .or_else(|| insert.state.as_deref().and_then(base64::decode));
                if let Some(state) = state {
                    instance.load_state(&state);
                }
                (instance, HashMap::new(), Vec::new())
            }
        };
        let (rate, max) = self.format;
        let mut processor = instance.activate(rate, max)?;
        processor.set_offline(self.offline);
        let latency = instance.latency();
        let (node, controls) = PluginNode::new(processor);
        let first = midi.is_some();
        let layered = layer.is_some();
        let node = engine.add_node(
            &descriptor.name,
            node.with_midi(midi).with_layer(layer),
            2,
            2,
        );
        let _ = engine.set_latency(node, latency);
        Ok(InsertNodes {
            id: insert.id,
            effect: insert.effect.clone(),
            node,
            first,
            layer: layered,
            files: insert.files.clone(),
            latency,
            declared: latency,
            kind: InsertKind::Plugin(PluginSlot {
                instance,
                controls,
                known,
                unsent,
                automation: HashMap::new(),
                latency,
            }),
        })
    }
}

/// A step of a channel's chain: an insert, or a parallel chain of branches.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    Insert(InsertId),
    Parallel(InsertId, Vec<Vec<Step>>),
}

/// A parallel chain: each branch ends in its level, all summed into `out`.
#[derive(Debug)]
struct ParallelNodes {
    id: InsertId,
    levels: Vec<(NodeId, SharedParam)>,
    out: NodeId,
}

/// Input stage, inserts and strip of a track or of the master.
#[derive(Debug)]
struct ChannelNodes {
    input: NodeId,
    input_gain: SharedParam,
    console: ConsoleControls,
    /// Every insert, those of parallel branches included.
    inserts: Vec<InsertNodes>,
    parallels: Vec<ParallelNodes>,
    /// How the inserts are wired.
    steps: Vec<Step>,
    strip: NodeId,
    controls: StripControls,
    /// MIDI of the first insert (tracks only).
    midi: Option<MidiControls>,
    /// The notes it plays, block by block, for the instruments layered on it.
    midi_share: Option<Arc<MidiShare>>,
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

/// A headphone mix: a send from each track (before its fader), from the click and from the
/// main mix, summed on its pair of outputs.
#[derive(Debug)]
struct CueNodes {
    out: NodeId,
    gain: SharedParam,
    output: u16,
    sends: HashMap<TrackId, (NodeId, SharedParam)>,
    click: (NodeId, SharedParam),
    main: (NodeId, SharedParam),
    /// Straight to the outputs, not ducked with the rest.
    talkback: (NodeId, SharedParam),
    /// The mix's volume, linear, before ducking.
    level: f32,
}

/// The talkback: a hardware input sent to the headphone mixes while pressed.
#[derive(Debug)]
struct Talkback {
    node: NodeId,
    gain: SharedParam,
    input: Option<usize>,
    on: bool,
    /// What the rest of each mix is lowered by while it is pressed, linear.
    duck: f32,
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
    /// Where the track records over what it plays (see [`RecordWindow`]).
    record_window: Arc<RecordWindow>,
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
    /// Nodes recording tracks after their effects.
    prints: Vec<NodeId>,
    cues: Vec<CueNodes>,
    talkback: Talkback,
    click_node: NodeId,
    control_room: crate::nodes::ControlRoomControls,
    recording: bool,
    preview: crate::nodes::PreviewControls,
    plugins: Plugins,
    /// Override flags of the automated controls, kept across automation updates.
    overrides: HashMap<(Channel, AutomationTarget), Arc<AtomicBool>>,
    /// Live MIDI input, read by the instruments of armed tracks.
    live: LiveMidi,
    /// Every console color off, shared by the input stages.
    console_off: Arc<AtomicBool>,
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
    fn new(
        engine: &mut Engine,
        solo: &SoloBus,
        bypass: &Arc<AtomicBool>,
        name: &str,
        master: bool,
    ) -> Self {
        let (input, input_gain, console) = InputStage::new(bypass.clone());
        let input = engine.add_node(&format!("{name} input"), input, 2, 2);
        let (strip, controls) = Strip::new(solo, master);
        let strip = engine.add_node(name, strip, 2, 2);
        Self {
            input,
            input_gain,
            console,
            inserts: Vec::new(),
            parallels: Vec::new(),
            steps: Vec::new(),
            strip,
            midi: None,
            midi_share: None,
            controls,
        }
    }

    /// The node a step ends with, if it was made.
    fn step_end(&self, step: &Step) -> Option<NodeId> {
        match step {
            Step::Insert(id) => self.inserts.iter().find(|n| n.id == *id).map(|n| n.node),
            Step::Parallel(id, _) => self.parallels.iter().find(|p| p.id == *id).map(|p| p.out),
        }
    }

    /// Last node before the fader, where pre-fader sends are taken.
    fn pre_fader(&self) -> NodeId {
        self.steps
            .iter()
            .rev()
            .find_map(|s| self.step_end(s))
            .unwrap_or(self.input)
    }

    /// Wires `steps` from `from`; returns the last node.
    fn wire(&self, steps: &[Step], from: NodeId, stereo: &mut dyn FnMut(NodeId, NodeId)) -> NodeId {
        let mut previous = from;
        for step in steps {
            match step {
                Step::Insert(id) => {
                    if let Some(n) = self.inserts.iter().find(|n| n.id == *id) {
                        stereo(previous, n.node);
                        previous = n.node;
                    }
                }
                Step::Parallel(id, branches) => {
                    let Some(p) = self.parallels.iter().find(|p| p.id == *id) else {
                        continue;
                    };
                    for (branch, (level, _)) in branches.iter().zip(&p.levels) {
                        let end = self.wire(branch, previous, stereo);
                        stereo(end, *level);
                        stereo(*level, p.out);
                    }
                    previous = p.out;
                }
            }
        }
        previous
    }

    fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        [self.input, self.strip]
            .into_iter()
            .chain(self.inserts.iter().map(|i| i.node))
            .chain(
                self.parallels
                    .iter()
                    .flat_map(|p| p.levels.iter().map(|l| l.0).chain([p.out])),
            )
    }
}

/// The console's index for the DSP: none when it colors nothing.
fn console_index(console: audiostrada_core::console::Console) -> usize {
    if console.colors() {
        console.model.index()
    } else {
        0
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
    // The members of an automated VCA: their gain follows its curve.
    for track in &project.tracks {
        if vca_lane(project, track).is_some() {
            out.insert((Channel::Track(track.id), AutomationTarget::Gain));
        }
    }
    out
}

/// The gain lane of `track`'s VCA, when it has one that plays.
fn vca_lane<'a>(
    project: &'a Project,
    track: &audiostrada_core::Track,
) -> Option<&'a audiostrada_core::automation::AutomationLane> {
    let vca = project.track(track.vca?)?;
    if vca.automation_mode == AutomationMode::Off {
        return None;
    }
    vca.automation
        .iter()
        .find(|l| l.target == AutomationTarget::Gain && l.enabled && !l.points.is_empty())
}

/// A track's own gain lane, when it plays.
fn own_gain_lane(
    track: &audiostrada_core::Track,
) -> Option<&[audiostrada_core::automation::AutomationPoint]> {
    if track.automation_mode == AutomationMode::Off {
        return None;
    }
    track
        .automation
        .iter()
        .find(|l| l.target == AutomationTarget::Gain && l.enabled && !l.points.is_empty())
        .map(|l| &l.points[..])
}

/// Parameter values of an insert, the channel strip taking its character from the console.
/// `fast`: without lookahead, as short as the effect allows (what is played, heard live).
fn insert_values(
    insert: &InsertModel,
    info: &audiostrada_dsp::EffectInfo,
    character: usize,
    fast: bool,
) -> Vec<f32> {
    let mut values = param_values(insert, info);
    if insert.effect == routing::STRIP
        && let Some(i) = info.params.iter().position(|p| p.id == "character")
    {
        values[i] = character as f32;
    }
    if fast {
        for (value, p) in values.iter_mut().zip(info.params) {
            if matches!(p.id, "lookahead" | "window") {
                *value = p.min;
            }
        }
    }
    values
}

/// What the low latency mode does to a channel's effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LowLatency {
    /// Not in the way of what is played.
    Keep,
    /// Bypassed when slower than the limit (on the way of what is played).
    Bypass,
    /// Kept without lookahead (the effects of what is played).
    Fast,
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
        let console_off = Arc::new(AtomicBool::new(false));
        let master = ChannelNodes::new(&mut engine, &solo, &console_off, "Master", true);
        let (meter, loudness) = LoudnessMeter::new();
        let meter = engine.add_node("Loudness", meter, 2, 2);
        connect_stereo(&mut engine, master.strip, meter);
        let out = engine.graph().hardware_output();
        // The reference track and the monitor inserts sit after the meters, on the way to the
        // outputs.
        let (reference, reference_controls) = crate::nodes::Reference::new();
        let reference = engine.add_node("Reference", reference, 2, 2);
        connect_stereo(&mut engine, meter, reference);
        let monitor = ChannelNodes::new(&mut engine, &solo, &console_off, "Monitor", true);
        connect_stereo(&mut engine, reference, monitor.input);
        let (room, control_room) = crate::nodes::ControlRoom::new();
        let room = engine.add_node("Control room", room, 2, 2);
        connect_stereo(&mut engine, monitor.strip, room);
        connect_stereo(&mut engine, room, out);
        let (preview, preview_controls) = crate::nodes::Preview::new();
        let preview = engine.add_node("Preview", preview, 0, 2);
        connect_stereo(&mut engine, preview, out);
        let (click, metronome) = crate::nodes::Metronome::new();
        let click = engine.add_node("Metronome", click, 0, 2);
        connect_stereo(&mut engine, click, out);
        let (talkback, talkback_gain) = Gain::new(0.0, None);
        let talkback = engine.add_node("Talkback", talkback, 2, 2);
        engine.set_unaligned(talkback);
        Self {
            auto_monitoring: false,
            prints: Vec::new(),
            cues: Vec::new(),
            talkback: Talkback {
                node: talkback,
                gain: talkback_gain,
                input: None,
                on: false,
                duck: 1.0,
            },
            click_node: click,
            control_room,
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
                restarting: Vec::new(),
                feedback: Vec::new(),
                resync: false,
                low_latency: None,
                forced: Vec::new(),
            },
            overrides: HashMap::new(),
            live: LiveMidi::default(),
            console_off,
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
        low_latency: LowLatency,
        character: usize,
    ) -> bool {
        let fast = low_latency == LowLatency::Fast && plugins.low_latency.is_some();
        let mut changed = false;
        changed |= Self::sync_parallels(engine, nodes, inserts, removed);
        let mut old = std::mem::take(&mut nodes.inserts);
        if inserts.is_empty() {
            nodes.midi = None;
            nodes.midi_share = None;
        }
        // Branch inserts after the channel's own: only the channel's first leads it.
        let lead = inserts.first().map(|i| i.id);
        // Instruments right after a leading instrument are layered on it: same notes, their
        // sounds added together, the effects after them processing the mix.
        let instrument = |effect: &str| {
            audiostrada_dsp::info(effect).map_or_else(
                || plugins.database.find(effect).is_some_and(|d| d.instrument),
                |i| i.instrument,
            )
        };
        let layers: Vec<InsertId> = if live.is_some()
            && inserts
                .first()
                .is_some_and(|i| i.effect != routing::PARALLEL && instrument(&i.effect))
        {
            inserts
                .iter()
                .skip(1)
                .take_while(|i| i.effect != routing::PARALLEL && instrument(&i.effect))
                .map(|i| i.id)
                .collect()
        } else {
            Vec::new()
        };
        for insert in routing::flatten(inserts)
            .into_iter()
            .filter(|i| i.effect != routing::PARALLEL)
        {
            let first = Some(insert.id) == lead && live.is_some();
            let layer = layers.contains(&insert.id);
            let existing = old
                .iter()
                .position(|n| {
                    n.id == insert.id
                        && n.effect == insert.effect
                        && n.first == first
                        && n.layer == layer
                        && n.files == insert.files
                        // Settings that change its delay make it again, compensated anew.
                        && audiostrada_dsp::info(&insert.effect).is_none_or(|info| {
                            audiostrada_dsp::declared_latency(
                                info.id,
                                &insert_values(insert, info, character, fast),
                                engine.sample_rate(),
                            )
                            .is_none_or(|l| l == n.latency)
                        })
                })
                .map(|i| old.remove(i));
            let mut midi = None;
            if existing.is_none()
                && first
                && let Some(live) = live
            {
                let (player, controls) = MidiPlayer::new(live.reader());
                let share = MidiShare::new();
                midi = Some(player.with_share(Arc::clone(&share)));
                nodes.midi = Some(controls);
                nodes.midi_share = Some(share);
            }
            let mut layer_midi = if existing.is_none() && layer {
                nodes.midi_share.clone().map(MidiLayer::new)
            } else {
                None
            };
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
                    // Its delay depends on its settings: apply them first.
                    for (i, v) in insert_values(insert, info, character, fast)
                        .into_iter()
                        .enumerate()
                    {
                        effect.set_param(i, v);
                    }
                    let latency = effect.latency();
                    let (node, controls) = Insert::new(
                        effect,
                        &insert_values(insert, info, character, fast),
                        info.meters.len(),
                    );
                    // Effects with a sidechain get two key inputs.
                    let inputs = if audiostrada_dsp::supports_sidechain(info.id) {
                        4
                    } else {
                        2
                    };
                    let node = engine.add_node(
                        info.name,
                        node.with_midi(midi.take()).with_layer(layer_midi.take()),
                        inputs,
                        2,
                    );
                    let _ = engine.set_latency(node, latency);
                    InsertNodes {
                        id: insert.id,
                        effect: insert.effect.clone(),
                        node,
                        kind: InsertKind::Builtin(controls),
                        first,
                        layer,
                        files: insert.files.clone(),
                        latency,
                        declared: latency,
                    }
                }
                (None, None) => {
                    if plugins.failed.contains_key(&insert.id) {
                        continue;
                    }
                    // Restarting in place: once its audio side is dropped (the graph without
                    // it committed, then collected), it is deactivated and activated again.
                    if let Some(r) = plugins.restarting.iter_mut().find(|r| r.id == insert.id) {
                        r.seen = true;
                        if !r.instance.try_deactivate() {
                            plugins.resync = true;
                            changed = true;
                            continue;
                        }
                    }
                    match plugins.create(engine, insert, midi.take(), layer_midi.take()) {
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
            let forced = low_latency == LowLatency::Bypass
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
                        let values = insert_values(insert, info, character, fast);
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

    /// Makes the nodes of the parallel chains (a level per branch, their sum) and the wiring
    /// of the channel; true if either changed. A bypassed chain is left out of the wiring.
    fn sync_parallels(
        engine: &mut Engine,
        nodes: &mut ChannelNodes,
        inserts: &[InsertModel],
        removed: &mut Vec<NodeId>,
    ) -> bool {
        fn steps(list: &[InsertModel]) -> Vec<Step> {
            list.iter()
                .filter(|i| !(i.effect == routing::PARALLEL && i.bypass))
                .map(|i| {
                    if i.effect == routing::PARALLEL {
                        Step::Parallel(i.id, i.branches.iter().map(|b| steps(&b.inserts)).collect())
                    } else {
                        Step::Insert(i.id)
                    }
                })
                .collect()
        }
        let mut changed = false;
        let mut old = std::mem::take(&mut nodes.parallels);
        for model in routing::flatten(inserts)
            .into_iter()
            .filter(|i| i.effect == routing::PARALLEL)
        {
            let count = model.branches.len();
            let parallel = match old
                .iter()
                .position(|p| p.id == model.id && p.levels.len() == count)
            {
                Some(at) => old.remove(at),
                None => {
                    changed = true;
                    let (out, _) = crate::nodes::Gain::new(1.0, None);
                    let out = engine.add_node("Parallel", out, 2, 2);
                    let levels = (0..count)
                        .map(|_| {
                            let (gain, param) = crate::nodes::Gain::new(1.0, None);
                            (engine.add_node("Branch", gain, 2, 2), param)
                        })
                        .collect();
                    ParallelNodes {
                        id: model.id,
                        levels,
                        out,
                    }
                }
            };
            // Muted branches, and the others when one is soloed, are silent.
            let solo = model.branches.iter().any(|b| b.solo);
            for (branch, (_, level)) in model.branches.iter().zip(&parallel.levels) {
                let heard = !branch.mute && (!solo || branch.solo);
                level.set(if heard {
                    db_to_gain(branch.gain_db)
                } else {
                    0.0
                });
            }
            nodes.parallels.push(parallel);
        }
        for gone in old {
            changed = true;
            removed.extend(gone.levels.iter().map(|l| l.0));
            removed.push(gone.out);
        }
        let wiring = steps(inserts);
        if wiring != nodes.steps {
            nodes.steps = wiring;
            changed = true;
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
        self.console_off
            .store(project.console_off, Ordering::Relaxed);
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
        // Folders the live tracks play through, on their way to the master.
        let mut live_path = HashSet::new();
        for track in project.tracks.iter().filter(|t| is_live(t)) {
            let mut at = track.as_ref();
            while at.master_send
                && let Some(parent) = at.parent.and_then(|p| project.track(p))
                && live_path.insert(parent.id)
            {
                at = parent;
            }
        }
        for track in project.tracks.iter().filter(|t| t.kind == TrackKind::Audio) {
            if !self.tracks.contains_key(&track.id) {
                let channel = ChannelNodes::new(
                    &mut self.engine,
                    &self.solo,
                    &self.console_off,
                    &track.name,
                    false,
                );
                let record_window = RecordWindow::new();
                let (player, player_controls) = ClipPlayer::new(Vec::new());
                let player = player.with_window(Arc::clone(&record_window));
                let player = self.engine.add_node(&track.name, player, 0, 2);
                let (monitor, monitor_gain, input_meter) = Monitor::new();
                let monitor = monitor.with_window(Arc::clone(&record_window));
                let monitor = self.engine.add_node(&track.name, monitor, 2, 2);
                self.engine.set_unaligned(monitor);
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
                        record_window,
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
            let console = audiostrada_core::console::of_track(project, track.id);
            // Fed by other tracks: it sums, as a console's bus.
            let bus = project.tracks.iter().any(|t| {
                (t.parent == Some(track.id) && t.master_send)
                    || t.sends.iter().any(|s| s.target == track.id)
            });
            nodes.channel.console.set(
                console_index(console),
                console.amount,
                bus,
                project.console_noise,
            );
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
                // What is played keeps its effects (an amp, for instance), and the tracks it
                // does not go through do not delay it.
                if is_live(track) {
                    LowLatency::Fast
                } else if live_path.contains(&track.id) {
                    LowLatency::Bypass
                } else {
                    LowLatency::Keep
                },
                console_index(console),
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
                        variations: Vec::new(),
                        take: None,
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
            LowLatency::Bypass,
            0,
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
            LowLatency::Bypass,
            console_index(project.console),
        );
        self.master.console.set(
            console_index(project.console),
            project.console.amount,
            true,
            project.console_noise,
        );
        for node in removed_nodes {
            self.remove_node(node);
        }
        topology |= self.sync_cues(project);
        for track in &project.tracks {
            let Some(nodes) = self.tracks.get(&track.id) else {
                continue;
            };
            let live = is_live(track);
            let ids: Vec<NodeId> = nodes
                .channel
                .nodes()
                .chain([nodes.player])
                .chain(nodes.sends.iter().map(|s| s.node))
                .chain(nodes.midi_out.iter().map(|m| m.node))
                .chain(
                    self.cues
                        .iter()
                        .filter_map(|c| c.sends.get(&track.id).map(|s| s.0)),
                )
                .collect();
            for id in ids {
                topology |= self.engine.set_live(id, live);
            }
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
        // A plugin restarting in place whose insert went away (or failed) goes with it.
        for mut r in std::mem::take(&mut self.plugins.restarting) {
            if std::mem::take(&mut r.seen) {
                self.plugins.restarting.push(r);
            } else {
                r.instance.close_gui();
                self.plugins.retired.push(r.instance);
            }
        }
    }

    /// Brings the headphone mixes in line with the project; true if nodes changed.
    fn sync_cues(&mut self, project: &Project) -> bool {
        let mut changed = false;
        while self.cues.len() > project.cues.len() {
            if let Some(cue) = self.cues.pop() {
                let nodes = [cue.out, cue.click.0, cue.main.0, cue.talkback.0]
                    .into_iter()
                    .chain(cue.sends.values().map(|s| s.0));
                for node in nodes.collect::<Vec<_>>() {
                    self.remove_node(node);
                }
                changed = true;
            }
        }
        let send = |engine: &mut Engine, name: &str| {
            let (node, gain) = Gain::new(0.0, None);
            (engine.add_node(name, node, 2, 2), gain)
        };
        while self.cues.len() < project.cues.len() {
            let (out, gain) = Gain::new(1.0, None);
            let out = self.engine.add_node("Headphones", out, 2, 2);
            // Each headphone mix goes out as it comes: what is played is never delayed there.
            self.engine.set_unaligned(out);
            let click = send(&mut self.engine, "Headphones click");
            let main = send(&mut self.engine, "Headphones main");
            let talkback = send(&mut self.engine, "Headphones talkback");
            self.engine.set_unaligned(talkback.0);
            self.cues.push(CueNodes {
                out,
                gain,
                output: 2,
                sends: HashMap::new(),
                click,
                main,
                talkback,
                level: 1.0,
            });
            changed = true;
        }
        let audio: Vec<TrackId> = project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .map(|t| t.id)
            .collect();
        for (cue, nodes) in project.cues.iter().zip(&mut self.cues) {
            if nodes.output != cue.output {
                nodes.output = cue.output;
                changed = true;
            }
            nodes.level = db_to_gain(cue.gain_db);
            let duck = if self.talkback.on {
                self.talkback.duck
            } else {
                1.0
            };
            nodes.gain.set(nodes.level * duck);
            nodes.talkback.1.set(db_to_gain(cue.talkback_db));
            nodes.click.1.set(cue.click_db.map_or(0.0, db_to_gain));
            nodes.main.1.set(if cue.follow_main { 1.0 } else { 0.0 });
            let gone: Vec<TrackId> = nodes
                .sends
                .keys()
                .filter(|id| !audio.contains(id))
                .copied()
                .collect();
            for id in gone {
                if let Some((node, _)) = nodes.sends.remove(&id) {
                    let _ = self.engine.remove_node(node);
                    self.edges.retain(|(a, b)| a.node != node && b.node != node);
                    changed = true;
                }
            }
            for &id in &audio {
                let (_, gain) = nodes.sends.entry(id).or_insert_with(|| {
                    changed = true;
                    send(&mut self.engine, "Headphones send")
                });
                let db = cue.level_db(id);
                let off = db <= audiostrada_core::model::MIN_GAIN_DB;
                gain.set(if off { 0.0 } else { db_to_gain(db) });
            }
        }
        changed
    }

    /// The talkback: hardware `input` heard in the headphone mixes while `on`, the rest of
    /// each mix lowered by `duck_db`. Never in the main mix, so never recorded nor exported.
    pub fn set_talkback(&mut self, input: Option<usize>, on: bool, duck_db: f32) {
        let input = input.map(|i| i.min(HARDWARE_CHANNELS - 1));
        if input != self.talkback.input {
            let hardware = self.engine.graph().hardware_input();
            for ch in 0..2 {
                if let Some(old) = self.talkback.input {
                    self.engine
                        .disconnect(Port::new(hardware, old), Port::new(self.talkback.node, ch));
                }
                if let Some(new) = input {
                    let _ = self
                        .engine
                        .connect(Port::new(hardware, new), Port::new(self.talkback.node, ch));
                }
            }
            self.talkback.input = input;
            self.engine.commit();
        }
        self.talkback.on = on && input.is_some();
        self.talkback.duck = db_to_gain(duck_db.min(0.0));
        self.talkback
            .gain
            .set(if self.talkback.on { 1.0 } else { 0.0 });
        let duck = if self.talkback.on {
            self.talkback.duck
        } else {
            1.0
        };
        for cue in &self.cues {
            cue.gain.set(cue.level * duck);
        }
    }

    pub fn talkback_on(&self) -> bool {
        self.talkback.on
    }

    /// The control room's Dim (-20 dB) and Mono, on the main output only.
    pub fn set_control_room(&mut self, dim: bool, mono: bool) {
        self.control_room.dim.store(dim, Ordering::Relaxed);
        self.control_room.mono.store(mono, Ordering::Relaxed);
    }

    pub fn control_room(&self) -> (bool, bool) {
        (
            self.control_room.dim.load(Ordering::Relaxed),
            self.control_room.mono.load(Ordering::Relaxed),
        )
    }

    /// Routing edges the project asks for.
    fn desired_edges(&self, project: &Project) -> HashSet<(Port, Port)> {
        let mut edges = HashSet::new();
        let hardware = self.engine.graph().hardware_output();
        for cue in &self.cues {
            let first = usize::from(cue.output).min(HARDWARE_CHANNELS - 2);
            for ch in 0..2 {
                edges.insert((Port::new(cue.out, ch), Port::new(hardware, first + ch)));
                edges.insert((
                    Port::new(cue.talkback.0, ch),
                    Port::new(hardware, first + ch),
                ));
                for (from, to) in [
                    (self.click_node, cue.click.0),
                    (cue.click.0, cue.out),
                    (self.monitor.strip, cue.main.0),
                    (cue.main.0, cue.out),
                    (self.talkback.node, cue.talkback.0),
                ] {
                    edges.insert((Port::new(from, ch), Port::new(to, ch)));
                }
                for (track, (node, _)) in &cue.sends {
                    if let Some(nodes) = self.tracks.get(track) {
                        let source = nodes.channel.pre_fader();
                        edges.insert((Port::new(source, ch), Port::new(*node, ch)));
                        edges.insert((Port::new(*node, ch), Port::new(cue.out, ch)));
                    }
                }
            }
        }
        let mut stereo = |from: NodeId, to: NodeId| {
            for ch in 0..2 {
                edges.insert((Port::new(from, ch), Port::new(to, ch)));
            }
        };
        let chain = |channel: &ChannelNodes, stereo: &mut dyn FnMut(NodeId, NodeId)| {
            let last = channel.wire(&channel.steps, channel.input, stereo);
            stereo(last, channel.strip);
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
            for insert in routing::flatten(inserts) {
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

    /// Records a track as its effects make it heard (before its fader) into `dir`: the take of
    /// its printed twin. `punch` is in project samples.
    pub fn start_print(
        &mut self,
        track: TrackId,
        name: String,
        stereo: bool,
        dir: PathBuf,
        punch: Option<(u64, u64)>,
    ) -> Result<Recorder, String> {
        let nodes = self.tracks.get(&track).ok_or("unknown track")?;
        let strip = nodes.channel.strip;
        let delay: u32 = nodes.channel.inserts.iter().map(|i| i.declared).sum();
        let sources: Vec<(Port, usize)> = self
            .engine
            .graph()
            .edges()
            .iter()
            .filter(|e| e.to.node == strip && e.to.channel < 2)
            .map(|e| (e.from, e.to.channel))
            .collect();
        let punch = punch.map(|(a, b)| (self.to_engine(a), self.to_engine(b)));
        let target = TakeTarget {
            track,
            name,
            inputs: if stereo { vec![0, 1] } else { vec![0] },
        };
        let rate = self.engine.sample_rate();
        let (node, recorder) =
            crate::record::start_node(target, dir, rate, punch, u64::from(delay))
                .map_err(|e| format!("cannot start recording: {e}"))?;
        let id = self.engine.add_node("Print", node, 2, 0);
        for (from, channel) in sources {
            let _ = self.engine.connect(from, Port::new(id, channel));
        }
        self.prints.push(id);
        self.engine.commit();
        Ok(recorder)
    }

    pub fn stop_recording(&mut self) {
        for id in std::mem::take(&mut self.prints) {
            self.remove_node(id);
        }
        self.engine.commit();
        self.engine.stop_recording();
        self.recording = false;
        self.set_record_window(&[], None);
        self.apply_monitoring();
    }

    /// Where `tracks` record over what they play, `start..end` in project samples (`end`
    /// `u64::MAX` until the recording stops): there they play nothing of their clips, and an
    /// automatically monitored input is heard only there. `None` closes every window.
    pub fn set_record_window(&mut self, tracks: &[TrackId], range: Option<(u64, u64)>) {
        let auto = self.auto_monitoring;
        for (id, nodes) in &self.tracks {
            match range.filter(|_| tracks.contains(id)) {
                Some((start, end)) => {
                    let end = if end == u64::MAX {
                        u64::MAX
                    } else {
                        self.to_engine(end)
                    };
                    let gates = auto && matches!(nodes.monitoring, Monitoring::Auto);
                    nodes.record_window.open(self.to_engine(start), end, gates);
                }
                None => nodes.record_window.close(),
            }
        }
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

    /// Low latency mode: effects adding more than 1 ms on the way of what is played (its
    /// folders and the master) are bypassed, so that it is heard at once. The other tracks'
    /// effects never delay it.
    pub fn set_low_latency(&mut self, on: bool) {
        self.set_low_latency_limit(on.then_some(1.0));
    }

    /// Low latency mode bypassing the effects slower than `ms`.
    pub fn set_low_latency_limit(&mut self, ms: Option<f32>) {
        let rate = self.engine.sample_rate() as f32;
        let limit = ms.map(|ms| (ms.max(0.0) * rate / 1000.0) as u32);
        if limit != self.plugins.low_latency {
            self.plugins.low_latency = limit;
            self.plugins.resync = true;
        }
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
                let mut heard = 0;
                while let Some((id, value)) = plugin.controls.from_plugin.try_recv() {
                    heard += 1;
                    plugin.known.insert(id, value);
                    plugin.instance.set_param(id, value);
                    self.plugins.feedback.push((channel, slot.id, id, value));
                }
                if heard > 0 {
                    tracing::debug!(heard, "plugin: values changed by the plugin itself");
                }
                // Values are held here and sent once the restarts are done: those coming with a
                // restart go to the audio side made after it, not into the queue of the one
                // going away.
                for request in plugin.instance.poll() {
                    match request {
                        audiostrada_plugins::PluginRequest::Restart => {
                            tracing::debug!("plugin: deactivated and activated again in place");
                            restarts.push((channel, slot.id))
                        }
                        audiostrada_plugins::PluginRequest::Crashed => {
                            crashes.push((channel, slot.id))
                        }
                        audiostrada_plugins::PluginRequest::ParamChanged(id, value) => {
                            plugin.hold(id, value);
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
            self.reactivate_plugin(channel, id);
        }
        for nodes in std::iter::once(&mut self.master)
            .chain(std::iter::once(&mut self.monitor))
            .chain(self.tracks.values_mut().map(|t| &mut t.channel))
        {
            for slot in &mut nodes.inserts {
                if let InsertKind::Plugin(plugin) = &mut slot.kind {
                    plugin.flush();
                }
            }
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

    /// Deactivates a plugin and activates the same instance again at the next syncs, once its
    /// audio side is dropped: its window stays open, its state as it is. What a plugin asks
    /// for to apply a preset (CLAP restart, VST3 latency or bus change).
    fn reactivate_plugin(&mut self, channel: Channel, id: InsertId) {
        let Some(nodes) = self.holder_mut(channel, id) else {
            return;
        };
        let Some(index) = nodes
            .inserts
            .iter()
            .position(|n| n.id == id && matches!(n.kind, InsertKind::Plugin(_)))
        else {
            return;
        };
        let slot = nodes.inserts.remove(index);
        let InsertKind::Plugin(plugin) = slot.kind else {
            return;
        };
        self.plugins.restarting.push(Restarting {
            id,
            instance: plugin.instance,
            known: plugin.known,
            unsent: plugin.unsent,
            seen: true,
        });
        self.remove_node(slot.node);
        self.plugins.resync = true;
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
            let follows_vca = match channel {
                Channel::Track(id) => project
                    .track(id)
                    .is_some_and(|t| vca_lane(project, t).is_some()),
                Channel::Master => false,
            };
            for lane in track_lanes {
                if !automated.contains(&(channel, lane.target.clone())) {
                    continue;
                }
                // Added to its VCA's curve below.
                if follows_vca && lane.target == AutomationTarget::Gain {
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
        // The members of an automated VCA: their gain (curve or level) plus the VCA's curve.
        let step = u64::from(self.project_rate.max(1)) / 50;
        for track in &project.tracks {
            let Some(vca) = vca_lane(project, track) else {
                continue;
            };
            let points = audiostrada_core::automation::sum(
                own_gain_lane(track),
                track.gain_db,
                &vca.points,
                step,
            );
            let channel = Channel::Track(track.id);
            let Some(nodes) = self.channel_mut(channel) else {
                continue;
            };
            let target = nodes.controls.gain_db.clone();
            let overridden = Arc::clone(
                self.overrides
                    .entry((channel, AutomationTarget::Gain))
                    .or_insert_with(|| Arc::new(AtomicBool::new(false))),
            );
            lanes.push(AutomationLane {
                points: points.into(),
                target,
                conversion: Conversion::Db { offset: 0.0 },
                overridden,
                ratio,
            });
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
    /// Panic: every instrument lets go of every note, and the effects fall silent.
    pub fn panic(&mut self) {
        for nodes in self.tracks.values() {
            if let Some(midi) = nodes.channel.midi.as_ref() {
                midi.panic();
            }
        }
        self.engine.flush();
    }

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
/// What is played on the track is heard live: armed, or always hearing its input.
fn is_live(track: &audiostrada_core::Track) -> bool {
    track.kind == TrackKind::Audio && (track.armed || track.monitor == Monitoring::Input)
}

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
    let insert = routing::find_insert(project, channel, *insert)?;
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
        let dir = audiostrada_core::scratch::dir(&format!("project-{name}"));
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

    /// Index of the loudest sample of each stretch over 0.1, in the left output.
    fn peaks(out: &[f32]) -> Vec<usize> {
        let mut found = Vec::new();
        let mut i = 0;
        while i < out.len() {
            if out[i].abs() > 0.1 {
                let end = (i + 64).min(out.len());
                let top = (i..end)
                    .max_by(|&a, &b| out[a].abs().total_cmp(&out[b].abs()))
                    .unwrap_or(i);
                found.push(top);
                i = end;
            } else {
                i += 1;
            }
        }
        found
    }

    /// Two tracks playing the same click, the first through a limiter; `live` arms the second.
    /// Returns the clicks of each track heard alone, over `blocks` blocks from the start.
    fn clicks(
        name: &str,
        live: bool,
        looping: Option<(u64, u64)>,
        blocks: usize,
    ) -> [Vec<usize>; 2] {
        let mut samples = vec![0.0; 4800];
        samples[150] = 0.8;
        let mut f = fixture(name, samples);
        let a = f.history.project().tracks[0].id;
        f.history.edit("Setup", |p| {
            routing::add_insert(p, Channel::Track(a), "limiter", Default::default()).unwrap();
            let source = *p.sources.keys().next().unwrap();
            let b = p.add_track("b");
            p.add_clip(b, source, 0);
            p.track_mut(b).unwrap().armed = live;
            p.loop_range = looping;
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        let mut heard = [Vec::new(), Vec::new()];
        for (solo, out) in heard.iter_mut().enumerate() {
            f.history.edit("Mute", |p| {
                for (i, t) in p.tracks.iter_mut().enumerate() {
                    Arc::make_mut(t).mute = i != solo;
                }
            });
            f.player.sync(f.history.project(), None, &f.media);
            f.player.engine_mut().stop();
            f.player.locate(0);
            f.player.engine_mut().flush();
            render(&mut processor, 20);
            f.player.engine_mut().play();
            *out = peaks(&render(&mut processor, blocks));
        }
        std::fs::remove_dir_all(&f.dir).unwrap();
        heard
    }

    #[test]
    fn live_tracks_play_back_aligned_with_the_rest() {
        for live in [false, true] {
            let [a, b] = clicks(&format!("aligned-{live}"), live, None, 40);
            assert_eq!(a.len(), 1, "{a:?}");
            assert_eq!(a, b, "armed: {live}");
        }
        let [a, b] = clicks("aligned-loop", true, Some((0, 1000)), 60);
        assert_eq!(a.len(), 4, "a click each pass: {a:?}");
        assert_eq!(a, b, "looping");
    }

    #[test]
    fn a_track_is_recorded_as_its_effects_make_it_in_line_with_its_input() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let media = MediaPool::new(48_000, None);
        let mut processor = player.processor(48_000, 64);
        let mut vox = TrackId(0);
        history.edit("Setup", |p| {
            vox = p.add_track("Vox");
            p.track_mut(vox).unwrap().armed = true;
            let gain = routing::add_insert(p, Channel::Track(vox), "utility", Default::default());
            routing::set_insert_param(p, Channel::Track(vox), gain.unwrap(), "gain", 6.0206);
            let comp =
                routing::add_insert(p, Channel::Track(vox), "compressor", Default::default())
                    .unwrap();
            for (param, value) in [
                ("ratio", 1.0),
                ("threshold", 0.0),
                ("color", 0.0),
                ("lookahead", 5.0),
            ] {
                routing::set_insert_param(p, Channel::Track(vox), comp, param, value);
            }
        });
        player.sync(history.project(), None, &media);
        player.update();
        let dir = audiostrada_core::scratch::dir("print");
        let _ = std::fs::remove_dir_all(&dir);
        let mut storage = ChannelBuffers::new(2, 2, 64);
        let mut n = 0u32;
        let mut block = |processor: &mut EngineProcessor, storage: &mut ChannelBuffers| {
            for i in 0..64 {
                storage.input_mut(0)[i] = ((n + i as u32) % 997) as f32 / 2000.0;
            }
            n += 64;
            processor.render(&mut storage.buffers(64));
        };
        for _ in 0..20 {
            block(&mut processor, &mut storage);
            player.update();
        }
        player.locate(6400);
        let punch = Some((6400, u64::MAX));
        let target = TakeTarget {
            track: vox,
            name: "Vox".into(),
            inputs: vec![0],
        };
        let raw = player
            .start_recording(vec![target], dir.clone(), punch)
            .unwrap();
        let print = player
            .start_print(vox, "Vox (printed)".into(), false, dir.clone(), punch)
            .unwrap();
        player.engine_mut().play();
        for _ in 0..60 {
            block(&mut processor, &mut storage);
            player.update();
        }
        player.stop_recording();
        block(&mut processor, &mut storage);
        player.update();
        let (raw, print) = (raw.finish(), print.finish());
        let (raw, print) = (&raw.takes[0], &print.takes[0]);
        assert_eq!(raw.start, print.start, "both start where the recording did");
        assert!(
            raw.frames - print.frames <= 240,
            "{} {}",
            raw.frames,
            print.frames
        );
        let a = audiostrada_media::load(&raw.path).unwrap();
        let b = audiostrada_media::load(&print.path).unwrap();
        let worst = (0..b.frames())
            .map(|i| (b.channel(0)[i] - 2.0 * a.channel(0)[i]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < 1e-3,
            "the print is the input through the chain, in line: {worst}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Two tracks playing a constant (0.5 and 0.25) with faders at -12 dB, and `cues`; returns
    /// the last value of the main left output and of outputs 2 and 4, playing.
    fn cue_levels(
        name: &str,
        cues: impl FnOnce(TrackId, TrackId) -> Vec<audiostrada_core::CueMix>,
        click: bool,
    ) -> [f32; 3] {
        let mut f = fixture(name, vec![0.5; 48_000]);
        let a = f.history.project().tracks[0].id;
        let quiet = f.dir.join("quiet.wav");
        write_wav(&quiet, &AudioData::new(vec![vec![0.25; 48_000]], 48_000)).unwrap();
        f.history.edit("Setup", |p| {
            let s = p.add_source(Source {
                path: quiet.clone(),
                channels: 1,
                sample_rate: 48_000,
                frames: 48_000,
            });
            let b = p.add_track("b");
            p.add_clip(b, s, 0);
            for t in p.tracks.iter_mut() {
                Arc::make_mut(t).gain_db = -12.0;
            }
            p.cues = cues(a, b);
            p.metronome.enabled = click;
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        let mut storage = ChannelBuffers::new(0, 6, 64);
        let mut peak = [0.0f32; 3];
        for _ in 0..40 {
            processor.render(&mut storage.buffers(64));
            for (p, ch) in peak.iter_mut().zip([0, 2, 4]) {
                *p = storage.output(ch).iter().fold(*p, |m, s| m.max(s.abs()));
            }
        }
        std::fs::remove_dir_all(&f.dir).unwrap();
        peak
    }

    #[test]
    fn headphone_mixes_have_their_own_levels_and_never_touch_the_main_mix() {
        use audiostrada_core::CueMix;
        use audiostrada_core::model::MIN_GAIN_DB;
        let cue = |levels: &[(TrackId, f32)], output| CueMix {
            output,
            levels: levels.iter().copied().collect(),
            click_db: None,
            ..CueMix::default()
        };
        let [main_alone, ..] = cue_levels("cue-none", |_, _| Vec::new(), false);
        let [main, both, only_a] = cue_levels(
            "cue-sum",
            |_, b| vec![cue(&[], 2), cue(&[(b, MIN_GAIN_DB)], 4)],
            false,
        );
        assert_eq!(
            main, main_alone,
            "the main mix is the same with headphone mixes"
        );
        assert!(
            (both - 0.75).abs() < 0.01,
            "both tracks before their faders: {both}"
        );
        assert!((only_a - 0.5).abs() < 0.01, "{only_a}");
        let [_, raised, _] = cue_levels(
            "cue-more-me",
            |_, b| {
                vec![CueMix {
                    more_me: Some(b),
                    more_me_db: 6.0206,
                    ..cue(&[], 2)
                }]
            },
            false,
        );
        assert!(
            (raised - 1.0).abs() < 0.01,
            "only its track is raised: {raised}"
        );
        let [_, click, none] = cue_levels(
            "cue-click",
            |a, b| {
                let off = [(a, MIN_GAIN_DB), (b, MIN_GAIN_DB)];
                vec![
                    CueMix {
                        click_db: Some(0.0),
                        ..cue(&off, 2)
                    },
                    cue(&off, 4),
                ]
            },
            true,
        );
        assert!(click > 0.05, "the click in the mix that has it: {click}");
        assert_eq!(none, 0.0, "and not in the other");
    }

    #[test]
    fn the_talkback_reaches_the_headphones_only_and_ducks_them() {
        let mut f = fixture("talkback", vec![0.5; 48_000]);
        f.history.edit("Cue", |p| {
            p.cues = vec![audiostrada_core::CueMix {
                click_db: None,
                ..Default::default()
            }];
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        let mut storage = ChannelBuffers::new(2, 4, 64);
        storage.input_mut(1).fill(0.3);
        let mut level = |player: &mut ProjectPlayer, on| {
            player.set_talkback(Some(1), on, -20.0);
            for _ in 0..10 {
                processor.render(&mut storage.buffers(64));
            }
            (storage.output(0)[63], storage.output(2)[63])
        };
        let (main, cue) = level(&mut f.player, false);
        assert!((cue - 0.5).abs() < 0.01, "the track alone: {cue}");
        let (main_on, cue_on) = level(&mut f.player, true);
        assert!(
            (cue_on - (0.05 + 0.3)).abs() < 0.01,
            "ducked, with the talkback: {cue_on}"
        );
        assert_eq!(main_on, main, "never in the main mix");
        let (_, after) = level(&mut f.player, false);
        assert!((after - 0.5).abs() < 0.01, "{after}");
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn dim_and_mono_change_the_main_output_only() {
        let mut f = fixture("control-room", vec![0.5; 48_000]);
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let t = f.history.project().tracks[0].id;
        f.history
            .edit("Pan", |p| p.track_mut(t).unwrap().pan = -1.0);
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        let mut storage = ChannelBuffers::new(0, 2, 64);
        let mut level = |player: &mut ProjectPlayer, dim, mono| {
            player.set_control_room(dim, mono);
            for _ in 0..10 {
                processor.render(&mut storage.buffers(64));
            }
            (storage.output(0)[63], storage.output(1)[63])
        };
        let (left, right) = level(&mut f.player, false, false);
        assert!(
            left > 0.1 && right.abs() < 1e-4,
            "panned left: {left} {right}"
        );
        let (dim, _) = level(&mut f.player, true, false);
        assert!((dim / left - 0.1).abs() < 0.01, "-20 dB: {}", dim / left);
        let (l, r) = level(&mut f.player, false, true);
        assert!(
            (l - r).abs() < 1e-6 && (l - left / 2.0).abs() < 1e-4,
            "mono: {l} {r}"
        );
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn a_slow_effect_on_a_played_track_does_not_delay_the_input_heard() {
        let mut f = fixture("live-input", vec![0.0; 4800]);
        let bed = f.history.project().tracks[0].id;
        f.history.edit("Setup", |p| {
            routing::add_insert(p, Channel::Track(bed), "limiter", Default::default()).unwrap();
            let vox = p.add_track("Vox");
            p.track_mut(vox).unwrap().armed = true;
            p.cues = vec![audiostrada_core::CueMix {
                click_db: None,
                ..Default::default()
            }];
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        let latency = f.player.engine().shared().latency();
        assert!(latency > 64, "the limiter delays playback: {latency}");
        let mut storage = ChannelBuffers::new(2, 4, 64);
        for _ in 0..10 {
            processor.render(&mut storage.buffers(64));
            f.player.update();
        }
        storage.input_mut(0)[0] = 0.8;
        let (mut out, mut cue) = (Vec::new(), Vec::new());
        for _ in 0..40 {
            processor.render(&mut storage.buffers(64));
            storage.input_mut(0)[0] = 0.0;
            out.extend_from_slice(storage.output(0));
            cue.extend_from_slice(storage.output(2));
        }
        assert_eq!(
            peaks(&out),
            [0],
            "heard at once, not after {latency} samples"
        );
        assert_eq!(peaks(&cue), [0], "in the headphones too");
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn effects_ring_out_after_a_stop_until_flushed() {
        let samples: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.07).sin() * 0.5).collect();
        let mut f = fixture("tails", samples);
        let t = f.history.project().tracks[0].id;
        f.history.edit("Reverb", |p| {
            let id = audiostrada_core::routing::add_insert(
                p,
                audiostrada_core::routing::Channel::Track(t),
                "reverb",
                Default::default(),
            )
            .unwrap();
            audiostrada_core::routing::set_insert_param(
                p,
                audiostrada_core::routing::Channel::Track(t),
                id,
                "decay",
                8.0,
            );
        });
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        f.player.sync(f.history.project(), None, &f.media);
        f.player.engine_mut().play();
        render(&mut processor, 75);
        // Stop and back to the start: the tail goes on.
        f.player.engine_mut().stop();
        f.player.locate(0);
        let energy = |x: &[f32]| x.iter().map(|s| s * s).sum::<f32>();
        let tail = render(&mut processor, 75);
        assert!(
            energy(&tail) > 1e-3,
            "the reverb rings out: {}",
            energy(&tail)
        );
        // Stopping again silences it.
        f.player.engine_mut().flush();
        render(&mut processor, 1);
        let after = render(&mut processor, 10);
        assert!(energy(&after) < 1e-9, "flushed: {}", energy(&after));
        std::fs::remove_dir_all(&f.dir).unwrap();
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
        let dir = audiostrada_core::scratch::dir("sidechain");
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
    fn low_latency_bypasses_slow_effects_on_the_way_of_what_is_played() {
        let mut history = History::new(Project::new("Test", 48_000));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let media = MediaPool::new(48_000, None);
        let _processor = player.processor(48_000, 64);
        let mut inserts = Vec::new();
        let mut vox = TrackId(0);
        history.edit("Tracks", |p| {
            let bus = p.add_track("Vocals");
            vox = p.add_track("Vox");
            routing::set_parent(p, vox, Some(bus)).unwrap();
            p.track_mut(vox).unwrap().armed = true;
            let bed = p.add_track("Bed");
            for channel in [
                Channel::Track(vox),
                Channel::Track(bus),
                Channel::Track(bed),
                Channel::Master,
            ] {
                inserts
                    .push(routing::add_insert(p, channel, "limiter", Default::default()).unwrap());
            }
        });
        player.sync(history.project(), None, &media);
        assert!(player.engine().shared().latency() > 0);
        let slow = player.insert_latency(Channel::Track(vox), inserts[0]);
        player.set_low_latency(true);
        assert!(player.take_resync());
        player.sync(history.project(), None, &media);
        let fast = player.insert_latency(Channel::Track(vox), inserts[0]);
        assert!(
            fast < slow,
            "the played track's limiter looks ahead less: {fast} < {slow}"
        );
        let mut bypassed = player.low_latency_bypassed().to_vec();
        bypassed.sort();
        assert_eq!(
            bypassed,
            [inserts[1], inserts[3]],
            "its folder and the master; the played track and the others keep theirs"
        );
        history.edit("Disarm", |p| p.track_mut(vox).unwrap().armed = false);
        player.sync(history.project(), None, &media);
        assert_eq!(player.low_latency_bypassed(), [inserts[3]]);
        player.set_low_latency(false);
        player.sync(history.project(), None, &media);
        assert!(player.low_latency_bypassed().is_empty());
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
    fn a_plugin_insert_plays_on_every_system() {
        let library = audiostrada_test_plugin::built_library()
            .expect("the test plugin is built with the tests");
        let dir = audiostrada_core::scratch::dir("engine-clap");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(library, dir.join("test-gain.clap")).unwrap();
        let mut database = Database::default();
        database.refresh(std::slice::from_ref(&dir), audiostrada_plugins::scan_file);
        // The system's folders are scanned too: the test plugin by its id.
        let plugin = database
            .plugins()
            .find(|p| p.id == "com.audiostrada.test-gain")
            .cloned()
            .expect("the test plugin");
        let mut f = fixture("test-plugin", vec![0.5; 48_000]);
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
        let out = render(&mut processor, 40);
        f.player.update();
        assert!(
            close(out[out.len() - 1], 0.5),
            "0 dB: {}",
            out[out.len() - 1]
        );
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
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_console_colors_and_compares() {
        let sine: Vec<f32> = (0..48_000).map(|i| 0.7 * (i as f32 * 0.05).sin()).collect();
        let mut f = fixture("console", sine);
        f.media.retain(&media_paths(f.history.project(), None));
        assert!(f.media.wait(Duration::from_secs(10)));
        let mut processor = f.player.processor(48_000, 64);
        let play = |f: &mut Fixture, processor: &mut EngineProcessor| {
            f.player.sync(f.history.project(), None, &f.media);
            f.player.locate(0);
            f.player.engine_mut().play();
            render(processor, 40)
        };
        let clean = play(&mut f, &mut processor);
        f.history.edit("Console", |p| {
            p.console = audiostrada_core::console::Console {
                model: audiostrada_core::console::ConsoleModel::Discrete,
                amount: 100.0,
            };
        });
        let colored = play(&mut f, &mut processor);
        let differs = |a: &[f32], b: &[f32]| {
            a[1_000..]
                .iter()
                .zip(&b[1_000..])
                .any(|(x, y)| (x - y).abs() > 1e-3)
        };
        assert!(differs(&clean, &colored));
        // Switched off: as clean.
        f.history.edit("Console", |p| p.console_off = true);
        let compared = play(&mut f, &mut processor);
        assert!(!differs(&clean, &compared));
        f.history.edit("Console", |p| p.console_off = false);
        // A track of its own stays clean.
        f.history.edit("Console", |p| {
            let t = p.tracks[0].id;
            p.track_mut(t).unwrap().console = Some(Default::default());
            p.console.model = audiostrada_core::console::ConsoleModel::Clean;
        });
        let own = play(&mut f, &mut processor);
        assert!(!differs(&clean, &own));
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn a_compressor_s_lookahead_sets_its_latency() {
        let mut f = fixture("comp-latency", vec![0.5; 4_800]);
        let t = f.history.project().tracks[0].id;
        let c = Channel::Track(t);
        let mut id = None;
        f.history.edit("Comp", |p| {
            id = routing::add_insert(p, c, "compressor", Default::default());
            routing::set_insert_param(p, c, id.unwrap(), "lookahead", 5.0);
        });
        let id = id.unwrap();
        f.player.sync(f.history.project(), None, &f.media);
        assert_eq!(f.player.insert_latency(c, id), 240);
        f.history.edit("Comp", |p| {
            routing::set_insert_param(p, c, id, "lookahead", 10.0);
            routing::set_insert_param(p, c, id, "type", 1.0);
        });
        f.player.sync(f.history.project(), None, &f.media);
        assert_eq!(
            f.player.insert_latency(c, id),
            480 + audiostrada_dsp::compressor::COLOR_LATENCY
        );
        std::fs::remove_dir_all(&f.dir).unwrap();
    }

    #[test]
    fn parallel_branches_add_up() {
        let chain = |edit: fn(&mut Project, Channel, InsertId)| {
            move |p: &mut Project, t: TrackId| {
                let c = Channel::Track(t);
                let par = routing::add_parallel(p, c, 0).unwrap();
                edit(p, c, par);
            }
        };
        // Two branches passing the signal: twice as loud.
        let both = level("parallel-both", chain(|_, _, _| {}));
        assert!(close(both, 1.0), "{both}");
        let muted = level(
            "parallel-mute",
            chain(|p, c, par| routing::branch_mut(p, c, par, 1).unwrap().mute = true),
        );
        assert!(close(muted, 0.5), "{muted}");
        let lower = level(
            "parallel-gain",
            chain(|p, c, par| routing::branch_mut(p, c, par, 0).unwrap().gain_db = -6.0206),
        );
        assert!(close(lower, 0.75), "{lower}");
        // An effect in a branch plays only there.
        let halved = level(
            "parallel-effect",
            chain(|p, c, par| {
                let u = routing::add_insert_at(p, c, (Some((par, 1)), 0), "utility").unwrap();
                routing::set_insert_param(p, c, u, "gain", -6.0206);
            }),
        );
        assert!(close(halved, 0.75), "{halved}");
        // Bypassed, the chain is left out: the signal goes through once.
        let bypassed = level(
            "parallel-bypass",
            chain(|p, c, par| routing::set_bypass(p, c, par, true)),
        );
        assert!(close(bypassed, 0.5), "{bypassed}");
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
        // A VCA's curve moves the tracks it controls, added to their own.
        let vca = level("auto-vca", |p, t| {
            let v = routing::add_vca(p, "VCA");
            routing::set_vca(p, t, Some(v)).unwrap();
            lane_mut(p.track_mut(v).unwrap(), &AutomationTarget::Gain).add(
                0,
                -6.0206,
                CurveShape::Linear,
            );
        });
        assert!(close(vca, 0.25), "{vca}");
        let both = level("auto-vca-own", |p, t| {
            let v = routing::add_vca(p, "VCA");
            routing::set_vca(p, t, Some(v)).unwrap();
            lane_mut(p.track_mut(v).unwrap(), &AutomationTarget::Gain).add(
                0,
                -6.0206,
                CurveShape::Linear,
            );
            lane_mut(p.track_mut(t).unwrap(), &AutomationTarget::Gain).add(
                0,
                -6.0206,
                CurveShape::Linear,
            );
        });
        assert!(close(both, 0.125), "{both}");

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
    fn instruments_after_the_first_are_layered_on_it() {
        // A synth, then the same synth again, then the same once more but bypassed: the
        // second plays the same note over the first; the third stays out.
        let level = |synths: usize, bypass_last: bool| {
            let mut history = History::new(Project::new("Test", 48_000));
            let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
            let media = MediaPool::new(48_000, None);
            let mut processor = player.processor(48_000, 64);
            history.edit("Synths", |p| {
                let t = p.add_track("Keys");
                for k in 0..synths {
                    let id = routing::add_insert(p, Channel::Track(t), "synth", Default::default());
                    if bypass_last && k + 1 == synths {
                        routing::set_bypass(p, Channel::Track(t), id.unwrap(), true);
                    }
                }
                let note = audiostrada_core::Note {
                    start: 0,
                    length: 48_000,
                    key: 60,
                    velocity: 110,
                    channel: 0,
                    ..Default::default()
                };
                p.add_midi_clip(t, 0, 48_000, vec![note]);
            });
            player.sync(history.project(), None, &media);
            player.engine_mut().play();
            let out = render(&mut processor, 300);
            (out[4800..].iter().map(|s| s * s).sum::<f32>() / (out.len() - 4800) as f32).sqrt()
        };
        let one = level(1, false);
        let two = level(2, false);
        assert!(one > 0.01, "the lead plays: {one}");
        assert!(
            two > one * 1.3,
            "the second adds to the first: {one} then {two}"
        );
        // A bypassed layer leaves the lead alone.
        let bypassed = level(2, true);
        assert!((bypassed / one - 1.0).abs() < 0.05, "{one} {bypassed}");
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

    /// Choosing another audio device replaces the processor (here after playing on the first
    /// one, at the same rate or another): the clips play on the new one.
    #[test]
    fn clips_play_after_the_device_changes() {
        for (rate, frames) in [(48_000, 256), (44_100, 256), (96_000, 128)] {
            let mut f = fixture(&format!("device-{rate}"), vec![0.25; 48_000]);
            f.media.retain(&media_paths(f.history.project(), None));
            assert!(f.media.wait(Duration::from_secs(10)));
            let mut first = f.player.processor(48_000, 480);
            f.player.sync(f.history.project(), None, &f.media);
            f.player.engine_mut().play();
            let out = render(&mut first, 50);
            assert!(out.iter().any(|&s| s != 0.0), "plays on the first device");
            f.player.engine_mut().stop();
            render(&mut first, 2);
            drop(first);

            // As Document::processor, then its syncs at each frame.
            f.media.set_rate(rate);
            let mut second = f.player.processor(rate, frames);
            f.player.sync(f.history.project(), None, &f.media);
            assert!(f.media.wait(Duration::from_secs(10)));
            f.media.poll();
            f.player.sync(f.history.project(), None, &f.media);
            f.player.engine_mut().locate(0);
            f.player.engine_mut().play();
            let out = render(&mut second, 100);
            assert!(
                out.iter().any(|&s| s != 0.0),
                "silent after moving to a {rate} Hz device"
            );
            std::fs::remove_dir_all(&f.dir).unwrap();
        }
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
