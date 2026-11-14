//! The engine handle (UI side) and its real-time processor (audio side).
//!
//! The handle edits the graph and sends compiled schedules and transport commands through a
//! lock-free queue. Replaced schedules come back through a garbage queue so that nothing is
//! freed on the audio thread. When the processor is dropped (stream closed), it returns its
//! processors so that they survive a device change.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use audiostrada_io::{AudioBuffers, AudioCallback, CallbackTime};
use audiostrada_rt::{Receiver, Sender, channel, permit_alloc};

use crate::automation::AutomationSet;
use crate::graph::{Graph, GraphError, NodeId, NodeKind, Port};
use crate::pool::WorkerPool;
use crate::record::{self, Capture, Recorder, TakeTarget};
use crate::schedule::{Schedule, compile};
use crate::tap::{InputTap, TapWriter};
use crate::tempo::TempoMap;
use crate::transport::{EngineShared, Transport};
use crate::{Processor, TransportInfo};

const QUEUE_CAPACITY: usize = 256;

enum Command {
    Schedule(Box<Schedule>),
    Play,
    /// Plays after a count-in of so many frames.
    CountIn(u64),
    Stop,
    Locate(u64),
    /// Empties the effects: tails, delays, held voices.
    Flush,
    Loop(Option<(u64, u64)>),
    Tempo(Arc<TempoMap>),
    Capture(Option<Box<Capture>>),
    Tap(Option<Box<TapWriter>>),
    Automation(Box<AutomationSet>),
}

enum Garbage {
    Schedule(Box<Schedule>),
    Tempo(Arc<TempoMap>),
    Capture(Box<Capture>),
    Tap(Box<TapWriter>),
    Automation(Box<AutomationSet>),
    /// Final schedule of a dropped processor, with the processors to give back to the graph.
    Returned(Box<Schedule>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EngineConfig {
    /// Worker threads besides the audio thread; `None` picks one per spare core.
    pub workers: Option<usize>,
}

impl EngineConfig {
    fn worker_count(&self) -> usize {
        self.workers.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get().saturating_sub(1))
                .unwrap_or(0)
                .min(7)
        })
    }
}

pub struct Engine {
    graph: Graph,
    /// Index in the audio thread's schedule of every node whose processor lives there.
    running: HashMap<NodeId, usize>,
    commands: Option<Sender<Command>>,
    garbage: Option<Receiver<Garbage>>,
    shared: Arc<EngineShared>,
    tempo: Arc<TempoMap>,
    transport: Transport,
    sample_rate: u32,
    max_frames: usize,
    config: EngineConfig,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("graph", &self.graph)
            .field("sample_rate", &self.sample_rate)
            .finish_non_exhaustive()
    }
}

impl Engine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            graph: Graph::new(),
            running: HashMap::new(),
            commands: None,
            garbage: None,
            shared: Arc::new(EngineShared::default()),
            tempo: Arc::new(TempoMap::default()),
            transport: Transport::default(),
            sample_rate: 48_000,
            max_frames: 1024,
            config,
        }
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn shared(&self) -> &Arc<EngineShared> {
        &self.shared
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Adds a node; it starts processing at the next [`commit`](Self::commit).
    pub fn add_node(
        &mut self,
        name: &str,
        processor: impl Processor,
        inputs: usize,
        outputs: usize,
    ) -> NodeId {
        self.graph.add(name, Box::new(processor), inputs, outputs)
    }

    pub fn remove_node(&mut self, id: NodeId) -> Result<(), GraphError> {
        self.graph.remove(id)
    }

    pub fn connect(&mut self, from: Port, to: Port) -> Result<(), GraphError> {
        self.graph.connect(from, to)
    }

    pub fn disconnect(&mut self, from: Port, to: Port) -> bool {
        self.graph.disconnect(from, to)
    }

    /// Declares the latency a node adds, for plugin delay compensation.
    pub fn set_latency(&mut self, id: NodeId, samples: u32) -> Result<(), GraphError> {
        self.graph.set_latency(id, samples)
    }

    /// Marks a node as carrying what is played live: no slower path delays it, and the
    /// players feeding it read ahead instead. Returns true if it changed.
    pub fn set_live(&mut self, id: NodeId, live: bool) -> bool {
        self.graph.set_live(id, live)
    }

    /// The node's output is mixed as it comes, never delayed nor waited for (input monitors).
    pub fn set_unaligned(&mut self, id: NodeId) {
        self.graph.set_unaligned(id);
    }

    /// Prepares new processors for the current format.
    fn prepare_pending(&mut self) {
        let (rate, frames) = (self.sample_rate, self.max_frames);
        for spec in self.graph.nodes.values_mut() {
            if let Some(processor) = &mut spec.processor {
                processor.prepare(rate, frames);
            }
        }
    }

    fn running_map(schedule: &Schedule) -> HashMap<NodeId, usize> {
        schedule
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.kind == NodeKind::Processor && (n.processor.is_some() || n.previous.is_some())
            })
            .map(|(i, n)| (n.id, i))
            .collect()
    }

    /// Sends the current graph to the audio thread.
    pub fn commit(&mut self) {
        self.collect_garbage();
        if self.commands.is_none() {
            return;
        }
        self.prepare_pending();
        let schedule = compile(&mut self.graph, &self.running, self.max_frames);
        let running = Self::running_map(&schedule);
        self.shared.set_latency(schedule.latency);
        if let Some(commands) = &mut self.commands {
            match commands.try_send(Command::Schedule(Box::new(schedule))) {
                Ok(()) => self.running = running,
                Err(Command::Schedule(mut schedule)) => {
                    tracing::error!("engine command queue full, graph change dropped");
                    self.give_back(&mut schedule);
                }
                Err(_) => {}
            }
        }
    }

    fn send(&mut self, command: Command) {
        if let Some(commands) = &mut self.commands
            && commands.try_send(command).is_err()
        {
            tracing::error!("engine command queue full");
        }
    }

    pub fn play(&mut self) {
        self.transport.playing = true;
        self.send(Command::Play);
    }

    /// Plays once the metronome has counted `frames` in, the position held meanwhile.
    pub fn play_after_count_in(&mut self, frames: u64) {
        self.transport.playing = true;
        self.send(Command::CountIn(frames));
    }

    pub fn stop(&mut self) {
        self.transport.playing = false;
        self.send(Command::Stop);
    }

    /// Moves the transport; the effects keep ringing, as when stopping (see [`Self::flush`]).
    pub fn locate(&mut self, position: u64) {
        self.transport.position = position;
        self.send(Command::Locate(position));
    }

    /// Silences what the effects still hold: reverb and delay tails, frozen loops, voices.
    pub fn flush(&mut self) {
        self.send(Command::Flush);
    }

    pub fn set_loop(&mut self, range: Option<(u64, u64)>) {
        self.transport.looping = range;
        self.send(Command::Loop(range));
    }

    pub fn looping(&self) -> Option<(u64, u64)> {
        self.transport.looping
    }

    pub fn set_tempo_map(&mut self, map: TempoMap) {
        self.tempo = Arc::new(map);
        let tempo = Arc::clone(&self.tempo);
        self.send(Command::Tempo(tempo));
    }

    /// Starts recording `targets` into WAV files in `dir`, only inside `punch` if given.
    pub fn start_recording(
        &mut self,
        targets: Vec<TakeTarget>,
        dir: PathBuf,
        punch: Option<(u64, u64)>,
    ) -> Result<Recorder, String> {
        if self.commands.is_none() {
            return Err("no audio stream is running".into());
        }
        let (capture, recorder) = record::start(targets, dir, self.sample_rate, punch)
            .map_err(|e| format!("cannot start recording: {e}"))?;
        self.send(Command::Capture(Some(capture)));
        Ok(recorder)
    }

    /// Replaces the automation played on the audio thread.
    pub fn set_automation(&mut self, set: AutomationSet) {
        self.send(Command::Automation(Box::new(set)));
    }

    /// Copies the hardware inputs `channels` for another thread to read, replacing the
    /// previous tap; `seconds` is how long the reader may wait between reads.
    pub fn open_input_tap(
        &mut self,
        channels: Vec<usize>,
        seconds: f32,
    ) -> Result<InputTap, String> {
        if self.commands.is_none() {
            return Err("no audio stream is running".into());
        }
        let (writer, tap) = crate::tap::open(channels, self.sample_rate, seconds);
        self.send(Command::Tap(Some(writer)));
        Ok(tap)
    }

    pub fn close_input_tap(&mut self) {
        self.send(Command::Tap(None));
    }

    /// Stops capturing; the recorder finishes once the capture comes back as garbage.
    pub fn stop_recording(&mut self) {
        self.send(Command::Capture(None));
    }

    pub fn playing(&self) -> bool {
        if self.commands.is_some() {
            self.shared.playing()
        } else {
            self.transport.playing
        }
    }

    pub fn position(&self) -> u64 {
        if self.commands.is_some() {
            self.shared.position()
        } else {
            self.transport.position
        }
    }

    /// Moves processors from a schedule back into the graph.
    fn give_back(&mut self, schedule: &mut Schedule) {
        for (id, processor) in schedule.take_processors() {
            if let Some(spec) = self.graph.nodes.get_mut(&id) {
                spec.processor = Some(processor);
            }
        }
    }

    /// Frees what the audio thread handed back. Call regularly from the UI thread.
    pub fn collect_garbage(&mut self) {
        let Some(garbage) = &mut self.garbage else {
            return;
        };
        let mut returned = Vec::new();
        for item in garbage.drain() {
            match item {
                Garbage::Schedule(schedule) => drop(schedule),
                Garbage::Tempo(tempo) => drop(tempo),
                Garbage::Capture(capture) => drop(capture),
                Garbage::Tap(tap) => drop(tap),
                Garbage::Automation(set) => drop(set),
                Garbage::Returned(schedule) => returned.push(schedule),
            }
        }
        let disconnected = garbage.is_disconnected();
        for mut schedule in returned {
            self.give_back(&mut schedule);
            self.running.clear();
        }
        if disconnected {
            self.garbage = None;
            self.commands = None;
        }
    }

    /// Creates the real-time processor for a stream; the previous one must have been dropped.
    pub fn processor(&mut self, sample_rate: u32, max_frames: usize) -> EngineProcessor {
        if let Some(position) = self.commands.as_ref().map(|_| self.shared.position()) {
            self.transport.position = position;
        }
        self.collect_garbage();
        if !self.running.is_empty() {
            tracing::warn!("previous engine processor still running; its nodes are reset");
            self.running.clear();
        }
        if sample_rate != self.sample_rate {
            // Positions are in samples: keep the same musical time at the new rate.
            let ratio = f64::from(sample_rate) / f64::from(self.sample_rate);
            let scale = |v: u64| (v as f64 * ratio).round() as u64;
            self.transport.position = scale(self.transport.position);
            self.transport.looping = self.transport.looping.map(|(a, b)| (scale(a), scale(b)));
        }
        self.sample_rate = sample_rate;
        self.max_frames = max_frames.max(1);
        self.prepare_pending();
        let schedule = compile(&mut self.graph, &self.running, self.max_frames);
        self.running = Self::running_map(&schedule);
        self.shared.set_latency(schedule.latency);
        self.shared
            .publish(self.transport.playing, self.transport.position);
        let (commands, commands_rx) = channel(QUEUE_CAPACITY);
        let (garbage_tx, garbage) = channel(QUEUE_CAPACITY);
        self.commands = Some(commands);
        self.garbage = Some(garbage);
        EngineProcessor {
            pool: WorkerPool::new(
                self.config.worker_count(),
                self.config.workers.is_none(),
                std::time::Duration::from_secs_f64(
                    self.max_frames as f64 / f64::from(sample_rate.max(1)),
                ),
            ),
            schedule: Some(Box::new(schedule)),
            commands: commands_rx,
            garbage: garbage_tx,
            transport: self.transport,
            tempo: Arc::clone(&self.tempo),
            shared: Arc::clone(&self.shared),
            sample_rate,
            capture: None,
            tap: None,
            automation: None,
            declick: Declick::new(sample_rate),
        }
    }
}

/// How a live stream takes a new graph without a click: its output fades out, the graph is
/// swapped in the silence, then the output fades back in.
struct Declick {
    /// The graph waiting for the silence.
    pending: Option<Box<Schedule>>,
    gain: f32,
    step: f32,
    /// Samples kept silent after a swap, while delay lines fill up again.
    hold: usize,
    /// Feeding a device: offline renders take a new graph at once.
    live: bool,
}

impl Declick {
    /// Fades of 5 ms.
    fn new(sample_rate: u32) -> Self {
        Self {
            pending: None,
            gain: 1.0,
            step: 1.0 / (sample_rate as f32 * 0.005).max(1.0),
            hold: 0,
            live: false,
        }
    }

    /// Ramps the outputs toward silence while a graph waits, keeps them silent while the new
    /// one fills its delay lines, then ramps back to full level.
    fn apply(&mut self, buffers: &mut AudioBuffers<'_>) {
        let target = if self.pending.is_some() { 0.0 } else { 1.0 };
        if self.gain == target && self.hold == 0 {
            return;
        }
        let step = if target > self.gain {
            self.step
        } else {
            -self.step
        };
        let (start, held) = (self.gain, self.hold);
        for channel in 0..buffers.output_channels() {
            let (mut gain, mut hold) = (start, held);
            for sample in buffers.output(channel) {
                if hold > 0 {
                    hold -= 1;
                } else {
                    gain = (gain + step).clamp(0.0, 1.0);
                }
                *sample *= gain;
            }
        }
        let frames = buffers.frames();
        let moving = frames.saturating_sub(held);
        self.hold = held.saturating_sub(frames);
        self.gain = (start + step * moving as f32).clamp(0.0, 1.0);
    }
}

pub struct EngineProcessor {
    pool: WorkerPool,
    schedule: Option<Box<Schedule>>,
    commands: Receiver<Command>,
    garbage: Sender<Garbage>,
    transport: Transport,
    tempo: Arc<TempoMap>,
    shared: Arc<EngineShared>,
    sample_rate: u32,
    capture: Option<Box<Capture>>,
    tap: Option<Box<TapWriter>>,
    automation: Option<Box<AutomationSet>>,
    declick: Declick,
}

impl std::fmt::Debug for EngineProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineProcessor")
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}

impl EngineProcessor {
    /// The worker threads take part (the graph is heavy, or their count was set).
    pub fn parallel(&self) -> bool {
        self.pool.parallel()
    }

    fn discard(&mut self, item: Garbage) {
        if let Err(item) = self.garbage.try_send(item) {
            // Last resort when the UI stopped collecting: free here rather than leak.
            permit_alloc(|| drop(item));
        }
    }

    fn take_schedule(&mut self, mut schedule: Box<Schedule>) {
        if let Some(mut old) = self.schedule.take() {
            let warmup = schedule.adopt(&mut old);
            if self.declick.live {
                self.declick.hold = warmup;
            }
            self.discard(Garbage::Schedule(old));
        }
        self.schedule = Some(schedule);
    }

    fn handle_commands(&mut self) {
        // A graph waiting for the silence holds back the commands sent after it.
        if self.declick.pending.is_some() {
            if self.declick.gain > 0.0 {
                return;
            }
            if let Some(schedule) = self.declick.pending.take() {
                self.take_schedule(schedule);
            }
        }
        while let Some(command) = self.commands.try_recv() {
            match command {
                Command::Schedule(schedule) => {
                    if self.declick.live && self.declick.gain > 0.0 && self.schedule.is_some() {
                        self.declick.pending = Some(schedule);
                        return;
                    }
                    self.take_schedule(schedule);
                }
                Command::Play => self.transport.playing = true,
                Command::CountIn(frames) => {
                    self.transport.playing = true;
                    self.transport.count = (frames > 0).then_some((frames, frames));
                }
                Command::Stop => {
                    self.transport.playing = false;
                    self.transport.count = None;
                }
                Command::Locate(position) => self.transport.position = position,
                Command::Flush => {
                    if let Some(schedule) = &mut self.schedule {
                        schedule.nodes.iter_mut().for_each(|n| n.reset());
                    }
                }
                Command::Loop(range) => self.transport.looping = range,
                Command::Tempo(tempo) => {
                    let old = std::mem::replace(&mut self.tempo, tempo);
                    self.discard(Garbage::Tempo(old));
                }
                Command::Automation(set) => {
                    if let Some(old) = self.automation.replace(set) {
                        self.discard(Garbage::Automation(old));
                    }
                }
                Command::Capture(capture) => {
                    if let Some(old) = std::mem::replace(&mut self.capture, capture) {
                        self.discard(Garbage::Capture(old));
                    }
                }
                Command::Tap(tap) => {
                    if let Some(old) = std::mem::replace(&mut self.tap, tap) {
                        self.discard(Garbage::Tap(old));
                    }
                }
            }
        }
    }

    /// Processes one block; usable directly for offline rendering.
    pub fn render(&mut self, buffers: &mut AudioBuffers<'_>) {
        self.handle_commands();
        buffers.clear_outputs();
        let Some(schedule) = self.schedule.as_deref_mut() else {
            return;
        };
        let (pool, tempo, sample_rate, playing, capture, tap, automation) = (
            &mut self.pool,
            &self.tempo,
            self.sample_rate,
            self.transport.playing,
            &mut self.capture,
            &mut self.tap,
            self.automation.as_deref(),
        );
        self.transport
            .advance(buffers.frames(), schedule.max_frames, |segment| {
                // Counting in, nothing moves but the clicks.
                let playing = playing && segment.counted.is_none();
                if let Some(automation) = automation {
                    automation.apply(segment.position);
                }
                if playing && let Some(capture) = capture.as_deref_mut() {
                    capture.write(buffers, segment.offset, segment.frames, segment.position);
                }
                if let Some(tap) = tap.as_deref_mut() {
                    let (offset, frames) = (segment.offset, segment.frames);
                    tap.write(buffers, offset, frames, segment.position, playing);
                }
                let beat = tempo.sample_to_beat(segment.position, sample_rate);
                let bbt = tempo.beat_to_bbt(beat);
                let signature = tempo.signature_at_beat(beat);
                let unit = 4.0 / f64::from(signature.denominator.max(1));
                let beat_start =
                    tempo.bbt_to_beat(audiostrada_core::tempo::BarBeatTick { tick: 0, ..bbt });
                let bpm = tempo.bpm_at_beat(beat);
                let bar = match segment.counted {
                    // Whole bars at the tempo where playing starts, from the first beat.
                    Some(counted) => {
                        let units = counted as f64 / (f64::from(sample_rate) * 60.0 / bpm * unit);
                        crate::BarPosition {
                            beat: (units.floor() as u32 % signature.numerator.max(1)) + 1,
                            fraction: units.fract(),
                            beats_per_bar: signature.numerator.max(1),
                            unit,
                        }
                    }
                    None => crate::BarPosition {
                        beat: bbt.beat,
                        fraction: ((beat - beat_start) / unit).clamp(0.0, 1.0),
                        beats_per_bar: signature.numerator.max(1),
                        unit,
                    },
                };
                let transport = TransportInfo {
                    playing,
                    position: segment.position,
                    sample_rate,
                    beat,
                    bpm,
                    bar,
                    counting: segment.counted.is_some(),
                };
                pool.run(
                    schedule,
                    buffers,
                    segment.offset,
                    segment.frames,
                    &transport,
                );
            });
        self.shared
            .publish(self.transport.playing, self.transport.position);
    }
}

impl AudioCallback for EngineProcessor {
    fn process(&mut self, buffers: &mut AudioBuffers<'_>, _time: &CallbackTime) {
        self.declick.live = true;
        self.render(buffers);
        self.declick.apply(buffers);
        if crate::evaluation::active() {
            let frames = buffers.frames();
            crate::evaluation::apply(
                buffers,
                frames,
                crate::evaluation::session(),
                self.sample_rate,
            );
        }
    }
}

impl Drop for EngineProcessor {
    fn drop(&mut self) {
        self.declick.gain = 0.0;
        self.handle_commands();
        if let Some(schedule) = self.schedule.take() {
            let _ = self.garbage.try_send(Garbage::Returned(schedule));
        }
    }
}
