//! Native PipeWire backend: a duplex DSP filter node linked to the selected devices.
//!
//! All PipeWire objects live on a dedicated thread running the PipeWire main loop; the backend
//! talks to it through commands. Audio runs on the PipeWire real-time data thread.

mod filter;
mod graph;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use pipewire as pw;
use pw::registry::{GlobalObject, Registry};
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;

use crate::routing::pair_ports;
use crate::stream::CallbackRunner;
use crate::{
    AudioBackend, AudioCallback, AudioStream, BackendKind, CallbackFactory, DeviceInfo, IoError,
    StreamConfig, StreamInfo, StreamStatus,
};
use filter::{FilterStream, MAX_FRAMES, START_FRAMES};
use graph::{Graph, Node, Port, parse_default_name};

const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const LINK_RETRY: Duration = Duration::from_millis(50);
const MAX_CHANNELS: u32 = 64;
const FALLBACK_RATE: u32 = 48_000;

type OpenReply = Result<u64, IoError>;
type Devices = (Option<u32>, Option<u32>);

enum Command {
    Open {
        config: StreamConfig,
        callback: Box<dyn AudioCallback>,
        status: Arc<StreamStatus>,
        info: StreamInfo,
        devices: Devices,
        reply: mpsc::Sender<OpenReply>,
    },
    Close {
        id: u64,
        done: mpsc::Sender<()>,
    },
    Quit,
}

fn lock(graph: &Mutex<Graph>) -> MutexGuard<'_, Graph> {
    graph.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Resolves devices and the stream format for `config`.
fn plan(graph: &Mutex<Graph>, config: &StreamConfig) -> Result<(StreamInfo, Devices), IoError> {
    if config.buffer_size == 0
        || config.buffer_size as usize > MAX_FRAMES
        || config.inputs > MAX_CHANNELS
        || config.outputs > MAX_CHANNELS
    {
        return Err(IoError::UnsupportedConfig(format!("{config:?}")));
    }
    let (info, devices) = {
        let graph = lock(graph);
        let resolve = |wanted: &Option<String>, default: Option<u32>| match wanted {
            Some(name) => graph
                .find_device(name)
                .map(Some)
                .ok_or_else(|| IoError::DeviceNotFound(name.clone())),
            None => Ok(default),
        };
        let input = match config.inputs {
            0 => None,
            _ => resolve(&config.input_device, graph.default_input())?,
        };
        let output = match config.outputs {
            0 => None,
            _ => resolve(&config.output_device, graph.default_output())?,
        };
        let describe = |node: Option<u32>| node.map_or("none", |n| graph.description(n));
        let info = StreamInfo {
            backend: BackendKind::PipeWire,
            device: format!("in: {}, out: {}", describe(input), describe(output)),
            sample_rate: config.sample_rate.or(graph.rate).unwrap_or(FALLBACK_RATE),
            buffer_size: config.buffer_size,
            inputs: config.inputs,
            outputs: config.outputs,
        };
        (info, (input, output))
    };
    Ok((info, devices))
}

/// Device a stream side should use after a graph change: none without channels, the default
/// when it follows it, else its own device while it exists.
fn route(
    graph: &Graph,
    channels: usize,
    follow: bool,
    current: Option<u32>,
    default: Option<u32>,
) -> Option<u32> {
    match (channels, follow) {
        (0, _) => None,
        (_, true) => default,
        (_, false) => current.filter(|&id| graph.has_node(id)),
    }
}

/// Links between the filter `node` and its devices as (output node, output port, input node,
/// input port); `None` until the filter's (inputs, outputs) ports are all in the graph.
fn links(
    graph: &Graph,
    node: u32,
    (inputs, outputs): (usize, usize),
    (input, output): Devices,
) -> Option<Vec<(u32, u32, u32, u32)>> {
    let ours_in = graph.node_ports(node, false);
    let ours_out = graph.node_ports(node, true);
    if ours_in.len() < inputs || ours_out.len() < outputs {
        return None;
    }
    let mut pairs = Vec::new();
    if let Some(device) = output {
        let theirs = graph.node_ports(device, false);
        pairs.extend(
            pair_ports(&theirs, &ours_out)
                .into_iter()
                .map(|(i, o)| (node, o, device, i)),
        );
    }
    if let Some(device) = input {
        let theirs = graph.node_ports(device, true);
        pairs.extend(
            pair_ports(&theirs, &ours_in)
                .into_iter()
                .map(|(o, i)| (device, o, node, i)),
        );
    }
    Some(pairs)
}

fn backend_error(message: impl ToString) -> IoError {
    IoError::backend(BackendKind::PipeWire, message)
}

/// Handle on the PipeWire thread, shared by the backend and its streams.
struct Connection {
    tx: pw::channel::Sender<Command>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Connection {
    fn send(&self, command: Command) -> Result<(), IoError> {
        self.tx
            .send(command)
            .map_err(|_| backend_error("PipeWire thread stopped"))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Quit);
        let thread = self
            .thread
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(thread) = thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct PipeWireBackend {
    connection: Arc<Connection>,
    graph: Arc<Mutex<Graph>>,
    device_generation: Arc<AtomicU64>,
}

impl fmt::Debug for PipeWireBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipeWireBackend").finish_non_exhaustive()
    }
}

impl PipeWireBackend {
    /// Connects to the PipeWire server and reads the current graph.
    pub fn connect() -> Result<Self, IoError> {
        let graph = Arc::new(Mutex::new(Graph::default()));
        let device_generation = Arc::new(AtomicU64::new(0));
        let (tx, rx) = pw::channel::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = {
            let graph = Arc::clone(&graph);
            let generation = Arc::clone(&device_generation);
            thread::Builder::new()
                .name("audiostrada-pipewire".into())
                .spawn(move || main_loop(graph, generation, rx, ready_tx))
                .map_err(backend_error)?
        };
        let connection = Arc::new(Connection {
            tx,
            thread: Mutex::new(Some(thread)),
        });
        match ready_rx.recv_timeout(REPLY_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                connection,
                graph,
                device_generation,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(backend_error("timed out connecting to PipeWire")),
        }
    }
}

impl AudioBackend for PipeWireBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::PipeWire
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, IoError> {
        Ok(lock(&self.graph).devices())
    }

    fn device_generation(&self) -> u64 {
        self.device_generation.load(Ordering::Relaxed)
    }

    fn open(
        &self,
        config: &StreamConfig,
        factory: &mut CallbackFactory<'_>,
    ) -> Result<Box<dyn AudioStream>, IoError> {
        let (info, devices) = plan(&self.graph, config)?;
        let callback = factory(&info);
        let status = Arc::new(StreamStatus::default());
        let (reply, reply_rx) = mpsc::channel();
        self.connection.send(Command::Open {
            config: config.clone(),
            callback,
            status: Arc::clone(&status),
            info: info.clone(),
            devices,
            reply,
        })?;
        let id = reply_rx
            .recv_timeout(REPLY_TIMEOUT)
            .map_err(|_| backend_error("timed out opening stream"))??;
        Ok(Box::new(PipeWireStream {
            id,
            info,
            status,
            connection: Arc::clone(&self.connection),
        }))
    }
}

struct PipeWireStream {
    id: u64,
    info: StreamInfo,
    status: Arc<StreamStatus>,
    connection: Arc<Connection>,
}

impl fmt::Debug for PipeWireStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipeWireStream")
            .field("id", &self.id)
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl AudioStream for PipeWireStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }

    fn status(&self) -> &StreamStatus {
        &self.status
    }
}

impl Drop for PipeWireStream {
    fn drop(&mut self) {
        let (done, done_rx) = mpsc::channel();
        if self
            .connection
            .send(Command::Close { id: self.id, done })
            .is_ok()
        {
            let _ = done_rx.recv_timeout(REPLY_TIMEOUT);
        }
    }
}

/// Objects owned by the PipeWire thread.
struct LoopState {
    core: pw::core::CoreRc,
    graph: Arc<Mutex<Graph>>,
    device_generation: Arc<AtomicU64>,
    streams: RefCell<HashMap<u64, FilterStream>>,
    next_id: Cell<u64>,
    metadata: RefCell<Vec<(pw::metadata::Metadata, pw::metadata::MetadataListener)>>,
}

fn main_loop(
    graph: Arc<Mutex<Graph>>,
    device_generation: Arc<AtomicU64>,
    rx: pw::channel::Receiver<Command>,
    ready: mpsc::Sender<Result<(), IoError>>,
) {
    pw::init();
    let setup = || -> Result<_, pw::Error> {
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        let registry = core.get_registry_rc()?;
        Ok((mainloop, context, core, registry))
    };
    let (mainloop, _context, core, registry) = match setup() {
        Ok(objects) => objects,
        Err(e) => {
            let _ = ready.send(Err(IoError::BackendUnavailable(BackendKind::PipeWire)));
            tracing::debug!("PipeWire connection failed: {e}");
            return;
        }
    };
    let state = Rc::new(LoopState {
        core: core.clone(),
        graph,
        device_generation,
        streams: RefCell::new(HashMap::new()),
        next_id: Cell::new(1),
        metadata: RefCell::new(Vec::new()),
    });

    let _registry_listener = {
        let registry_weak = registry.downgrade();
        let on_global = Rc::clone(&state);
        let on_remove = Rc::clone(&state);
        registry
            .add_listener_local()
            .global(move |global| {
                if let Some(registry) = registry_weak.upgrade() {
                    on_global.on_global(&registry, global);
                }
            })
            .global_remove(move |id| on_remove.on_global_remove(id))
            .register()
    };

    // Two roundtrips: the first lists globals, the second receives the metadata bound meanwhile.
    let ready = Cell::new(Some(ready));
    let pending = Cell::new(core.sync(0).ok());
    let roundtrips = Cell::new(0);
    let _core_listener = {
        let core_weak = core.downgrade();
        core.add_listener_local()
            .done(move |id, seq| {
                if id != pw::core::PW_ID_CORE || pending.get() != Some(seq) {
                    return;
                }
                roundtrips.set(roundtrips.get() + 1);
                if roundtrips.get() == 1 {
                    pending.set(core_weak.upgrade().and_then(|c| c.sync(0).ok()));
                } else if let Some(ready) = ready.take() {
                    let _ = ready.send(Ok(()));
                }
            })
            .error(|id, _seq, res, message| {
                tracing::warn!("PipeWire error on object {id}: {message} ({res})");
            })
            .register()
    };

    let link_timer = {
        let state = Rc::clone(&state);
        mainloop.loop_().add_timer(move |_| state.link_pending())
    };
    link_timer.update_timer(Some(LINK_RETRY), Some(LINK_RETRY));

    let _receiver = {
        let state = Rc::clone(&state);
        let mainloop_weak = mainloop.downgrade();
        rx.attach(mainloop.loop_(), move |command| match command {
            Command::Open {
                config,
                callback,
                status,
                info,
                devices,
                reply,
            } => {
                let _ = reply.send(state.open(&config, callback, status, &info, devices));
            }
            Command::Close { id, done } => {
                state.streams.borrow_mut().remove(&id);
                let _ = done.send(());
            }
            Command::Quit => {
                if let Some(mainloop) = mainloop_weak.upgrade() {
                    mainloop.quit();
                }
            }
        })
    };

    mainloop.run();
    state.streams.borrow_mut().clear();
    state.metadata.borrow_mut().clear();
}

impl LoopState {
    fn on_global_remove(&self, id: u32) {
        if lock(&self.graph).remove(id) {
            self.device_generation.fetch_add(1, Ordering::Relaxed);
            self.refresh_routes();
        }
    }

    fn on_global(self: &Rc<Self>, registry: &Registry, global: &GlobalObject<&DictRef>) {
        let Some(props) = global.props else {
            return;
        };
        match global.type_ {
            ObjectType::Node => {
                let name = props.get("node.name").unwrap_or_default();
                let description = props
                    .get("node.description")
                    .or_else(|| props.get("node.nick"))
                    .unwrap_or(name);
                let node = Node {
                    name: name.to_owned(),
                    description: description.to_owned(),
                    class: props.get("media.class").unwrap_or_default().to_owned(),
                };
                lock(&self.graph).nodes.insert(global.id, node);
                self.device_generation.fetch_add(1, Ordering::Relaxed);
            }
            ObjectType::Port => {
                let number = |key| props.get(key).and_then(|v| v.parse::<u32>().ok());
                let (Some(node), Some(index)) = (number("node.id"), number("port.id")) else {
                    return;
                };
                let port = Port {
                    node,
                    index,
                    output: props.get("port.direction") == Some("out"),
                    monitor: props.get("port.monitor") == Some("true"),
                };
                let is_device = {
                    let mut graph = lock(&self.graph);
                    graph.ports.insert(global.id, port);
                    graph.is_device(node)
                };
                if is_device {
                    self.refresh_routes();
                }
            }
            ObjectType::Metadata => {
                if !matches!(props.get("metadata.name"), Some("default" | "settings")) {
                    return;
                }
                let metadata: pw::metadata::Metadata = match registry.bind(global) {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        tracing::warn!("cannot bind PipeWire metadata: {e}");
                        return;
                    }
                };
                let graph = Arc::clone(&self.graph);
                let state: Weak<Self> = Rc::downgrade(self);
                let listener = metadata
                    .add_listener_local()
                    .property(move |_subject, key, _type, value| {
                        let mut graph = lock(&graph);
                        let previous = (graph.default_input(), graph.default_output());
                        match key {
                            Some("default.audio.sink") => {
                                graph.default_sink = value.and_then(parse_default_name);
                            }
                            Some("default.audio.source") => {
                                graph.default_source = value.and_then(parse_default_name);
                            }
                            Some("default.configured.audio.sink") => {
                                graph.configured_sink = value.and_then(parse_default_name);
                            }
                            Some("default.configured.audio.source") => {
                                graph.configured_source = value.and_then(parse_default_name);
                            }
                            Some("clock.rate") => graph.rate = value.and_then(|v| v.parse().ok()),
                            _ => {}
                        }
                        let changed = previous != (graph.default_input(), graph.default_output());
                        drop(graph);
                        if let Some(state) = state.upgrade().filter(|_| changed) {
                            state.refresh_routes();
                        }
                        0
                    })
                    .register();
                self.metadata.borrow_mut().push((metadata, listener));
            }
            _ => {}
        }
    }

    fn open(
        &self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
        status: Arc<StreamStatus>,
        info: &StreamInfo,
        devices: Devices,
    ) -> OpenReply {
        let mut props = pw::properties::properties! {
            "media.type" => "Audio",
            "media.category" => "Duplex",
            "media.role" => "DSP",
            "node.name" => "audiostrada",
            "node.description" => "AudioStrada",
            "node.latency" => format!("{}/{}", info.buffer_size.max(START_FRAMES), info.sample_rate),
            "node.autoconnect" => "false",
            "node.always-process" => "true",
            "node.want-driver" => "true",
        };
        if let Some(rate) = config.sample_rate {
            // A suggested rate only applies if the server allows it: force it while we run.
            props.insert("node.rate", format!("1/{rate}"));
            props.insert("node.force-rate", "0");
        }
        let runner = CallbackRunner::new(callback, status, info.sample_rate);
        let mut stream = FilterStream::new(
            &self.core,
            props,
            config.inputs as usize,
            config.outputs as usize,
            devices,
            (
                config.input_device.is_none(),
                config.output_device.is_none(),
            ),
            runner,
        )?;
        stream.ask(info.buffer_size, info.sample_rate);
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.streams.borrow_mut().insert(id, stream);
        Ok(id)
    }

    /// Re-targets streams after default or device changes, unlinking those whose devices changed.
    fn refresh_routes(&self) {
        let Ok(mut streams) = self.streams.try_borrow_mut() else {
            return;
        };
        let graph = lock(&self.graph);
        for stream in streams.values_mut() {
            let input = route(
                &graph,
                stream.inputs,
                stream.follow_input,
                stream.input_device,
                graph.default_input(),
            );
            let output = route(
                &graph,
                stream.outputs,
                stream.follow_output,
                stream.output_device,
                graph.default_output(),
            );
            if (input, output) == (stream.input_device, stream.output_device) {
                continue;
            }
            for link in stream.links.drain(..) {
                let _ = self.core.destroy_object(link);
            }
            tracing::info!(
                "AudioStrada rerouted to in: {}, out: {}",
                input.map_or("none", |n| graph.description(n)),
                output.map_or("none", |n| graph.description(n))
            );
            stream.input_device = input;
            stream.output_device = output;
            stream.linked = false;
        }
    }

    /// Links the ports of new filters to their devices once they appear in the graph.
    fn link_pending(&self) {
        let mut streams = self.streams.borrow_mut();
        for stream in streams.values_mut().filter(|s| s.linked) {
            stream.settle();
        }
        for stream in streams.values_mut().filter(|s| !s.linked) {
            let Some(node) = stream.node_id() else {
                continue;
            };
            let devices = (stream.input_device, stream.output_device);
            let Some(pairs) = links(
                &lock(&self.graph),
                node,
                (stream.inputs, stream.outputs),
                devices,
            ) else {
                continue;
            };
            stream.start_large();
            for (out_node, out_port, in_node, in_port) in pairs {
                let props = pw::properties::properties! {
                    "link.output.node" => out_node.to_string(),
                    "link.output.port" => out_port.to_string(),
                    "link.input.node" => in_node.to_string(),
                    "link.input.port" => in_port.to_string(),
                    "object.linger" => "false",
                };
                match self
                    .core
                    .create_object::<pw::link::Link>("link-factory", &props)
                {
                    Ok(link) => stream.links.push(link),
                    Err(e) => tracing::warn!("cannot link PipeWire ports: {e}"),
                }
            }
            stream.linked = true;
            tracing::info!(node, links = stream.links.len(), "AudioStrada node linked");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_resolves_devices_and_rate() {
        let mut g = graph::tests::graph();
        g.rate = Some(44_100);
        let graph = Mutex::new(g);
        let (info, devices) = plan(&graph, &StreamConfig::default()).unwrap();
        assert_eq!(devices, (Some(11), Some(10)));
        assert_eq!(info.device, "in: MIC, out: SINK");
        assert_eq!(info.sample_rate, 44_100);
        let config = StreamConfig {
            inputs: 0,
            output_device: Some("sink".into()),
            sample_rate: Some(96_000),
            ..StreamConfig::default()
        };
        let (info, devices) = plan(&graph, &config).unwrap();
        assert_eq!(devices, (None, Some(10)));
        assert_eq!(
            (info.device.as_str(), info.sample_rate),
            ("in: none, out: SINK", 96_000)
        );
        let missing = StreamConfig {
            input_device: Some("nope".into()),
            ..StreamConfig::default()
        };
        assert!(matches!(
            plan(&graph, &missing),
            Err(IoError::DeviceNotFound(name)) if name == "nope"
        ));
        for config in [
            StreamConfig {
                buffer_size: 0,
                ..StreamConfig::default()
            },
            StreamConfig {
                buffer_size: MAX_FRAMES as u32 + 1,
                ..StreamConfig::default()
            },
            StreamConfig {
                inputs: MAX_CHANNELS + 1,
                ..StreamConfig::default()
            },
        ] {
            assert!(matches!(
                plan(&graph, &config),
                Err(IoError::UnsupportedConfig(_))
            ));
        }
    }

    #[test]
    fn routes_follow_defaults_or_keep_their_device() {
        let g = graph::tests::graph();
        assert_eq!(route(&g, 0, true, Some(10), Some(10)), None);
        assert_eq!(route(&g, 2, true, Some(11), Some(10)), Some(10));
        assert_eq!(route(&g, 2, false, Some(11), Some(10)), Some(11));
        assert_eq!(route(&g, 2, false, Some(99), Some(10)), None);
    }

    #[test]
    fn links_wait_for_every_filter_port() {
        let mut g = graph::tests::graph();
        let port = |node, output, index| Port {
            node,
            output,
            monitor: false,
            index,
        };
        g.ports.insert(200, port(20, false, 0));
        g.ports.insert(210, port(20, true, 0));
        g.ports.insert(211, port(20, true, 1));
        assert_eq!(links(&g, 20, (2, 2), (Some(11), Some(10))), None);
        g.ports.insert(201, port(20, false, 1));
        let pairs = links(&g, 20, (2, 2), (Some(11), Some(10))).unwrap();
        assert_eq!(
            pairs,
            vec![
                (20, 210, 10, 100),
                (20, 211, 10, 101),
                (11, 110, 20, 200),
                (11, 110, 20, 201),
            ]
        );
        assert_eq!(links(&g, 20, (2, 2), (None, None)), Some(Vec::new()));
    }
}
