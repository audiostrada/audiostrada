//! Plugins in their own process: a crashing or hanging plugin cannot take AudioStrada down.
//!
//! Each instance runs in a `audiostrada plugin-host` child. Control calls (parameters, state, GUI)
//! travel as JSON lines over the child's stdin and stdout; audio, parameter changes and MIDI go
//! through shared memory, the engine waking the child's audio thread for each block and waiting
//! for its answer at most a block's duration.

mod shm;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    MidiEvent, ParamDesc, PluginDescriptor, PluginInstance, PluginProcessor, PluginRequest,
};
use shm::{Shared, Signal};

/// Subcommand of the AudioStrada executable that hosts one plugin.
pub const HOST_COMMAND: &str = "plugin-host";
/// Marks protocol lines on the child's stdout, which plugins may also print to.
const PREFIX: &str = "\u{1e}AUDIOSTRADA ";
const MAX_CHANGES: usize = 256;
const MAX_MIDI: usize = 512;
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const LOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Serialize, Deserialize)]
enum Request {
    Load(PluginDescriptor),
    Params,
    ParamValue(u32),
    ValueText(u32, f64),
    SetParam(u32, f64),
    Latency,
    SaveState,
    LoadState(Vec<u8>),
    Poll,
    HasGui,
    /// Title, the window to stay above and the keys to hand back (see `window`).
    OpenGui(String, u64, Vec<(u32, u16)>),
    CloseGui,
    RaiseGui,
    Activate {
        shm: String,
        sample_rate: u32,
        max_frames: usize,
    },
    Deactivate,
}

impl Request {
    /// For the log, without the payload (a state can weigh megabytes).
    fn name(&self) -> &'static str {
        match self {
            Self::Load(_) => "Load",
            Self::Params => "Params",
            Self::ParamValue(_) => "ParamValue",
            Self::ValueText(..) => "ValueText",
            Self::SetParam(..) => "SetParam",
            Self::Latency => "Latency",
            Self::SaveState => "SaveState",
            Self::LoadState(_) => "LoadState",
            Self::Poll => "Poll",
            Self::HasGui => "HasGui",
            Self::OpenGui(..) => "OpenGui",
            Self::CloseGui => "CloseGui",
            Self::RaiseGui => "RaiseGui",
            Self::Activate { .. } => "Activate",
            Self::Deactivate => "Deactivate",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum Response {
    Done,
    Failed(String),
    Params(Vec<ParamDesc>),
    Value(Option<f64>),
    Text(Option<String>),
    Latency(u32),
    State(Option<Vec<u8>>),
    Flag(bool),
    Requests(Vec<PluginRequest>),
}

#[repr(C)]
struct Header {
    request: AtomicU32,
    done: AtomicU32,
    frames: AtomicU32,
    params_in: AtomicU32,
    midi: AtomicU32,
    params_out: AtomicU32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Change {
    id: u32,
    _pad: u32,
    value: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Midi {
    time: u32,
    data: [u8; 4],
}

/// Byte offsets of the buffers in the shared region.
#[derive(Clone, Copy, Debug)]
struct Layout {
    max_frames: usize,
    left: usize,
    right: usize,
    params_in: usize,
    midi: usize,
    params_out: usize,
    len: usize,
}

impl Layout {
    fn new(max_frames: usize) -> Self {
        let align = |x: usize| x.next_multiple_of(64);
        let left = 64;
        let right = align(left + max_frames * 4);
        let params_in = align(right + max_frames * 4);
        let midi = align(params_in + MAX_CHANGES * size_of::<Change>());
        let params_out = align(midi + MAX_MIDI * size_of::<Midi>());
        let len = align(params_out + MAX_CHANGES * size_of::<Change>());
        Self {
            max_frames,
            left,
            right,
            params_in,
            midi,
            params_out,
            len,
        }
    }
}

/// The shared region of an active instance, seen from either process.
struct Region {
    shared: Shared,
    layout: Layout,
    request: Signal,
    done: Signal,
}

impl std::fmt::Debug for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Region")
            .field("shared", &self.shared)
            .finish()
    }
}

impl Region {
    fn new(shared: Shared, layout: Layout) -> Result<Self, String> {
        let request = shared.signal(0, "request")?;
        let done = shared.signal(4, "done")?;
        Ok(Self {
            shared,
            layout,
            request,
            done,
        })
    }

    fn header(&self) -> &Header {
        // SAFETY: the region starts with a Header, made only of atomics.
        unsafe { &*(self.shared.as_ptr() as *const Header) }
    }

    /// # Safety
    /// Only one side may use a buffer at a time, which the request/done handshake ensures.
    #[allow(
        clippy::mut_from_ref,
        reason = "shared memory handed over by the handshake"
    )]
    unsafe fn slice<T>(&self, offset: usize, len: usize) -> &mut [T] {
        // SAFETY: offsets come from the layout and stay within the mapping.
        unsafe { std::slice::from_raw_parts_mut(self.shared.as_ptr().add(offset).cast(), len) }
    }
}

/// Main-thread side of a plugin running in a child process.
pub struct SandboxInstance {
    descriptor: PluginDescriptor,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    responses: mpsc::Receiver<(u64, Response)>,
    next: u64,
    crashed: Arc<AtomicBool>,
    reported: bool,
    processor_alive: Arc<AtomicBool>,
    active: bool,
    gui_open: bool,
    last_state: Option<Vec<u8>>,
    /// Display texts already asked for, as the generic editor redraws them every frame.
    texts: HashMap<(u32, u64), Option<String>>,
}

impl std::fmt::Debug for SandboxInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxInstance")
            .field("plugin", &self.descriptor.name)
            .field("pid", &self.pid())
            .finish_non_exhaustive()
    }
}

impl SandboxInstance {
    /// Starts `exe plugin-host` and loads `descriptor` in it.
    pub fn new(exe: &Path, descriptor: &PluginDescriptor) -> Result<Self, String> {
        let mut child = Command::new(exe)
            .arg(HOST_COMMAND)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("cannot start the plugin host: {e}"))?;
        #[cfg(windows)]
        kill_with_audiostrada(&child);
        let stdout = child.stdout.take().ok_or("no plugin host output")?;
        let (tx, responses) = mpsc::channel();
        let name = descriptor.name.clone();
        std::thread::Builder::new()
            .name(format!("plugin-host {name}"))
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    match line.find(PREFIX) {
                        Some(at) => {
                            let message = &line[at + PREFIX.len()..];
                            match serde_json::from_str::<(u64, Response)>(message) {
                                Ok(r) => {
                                    if tx.send(r).is_err() {
                                        break;
                                    }
                                }
                                Err(e) => tracing::warn!("{name}: bad host message: {e}"),
                            }
                        }
                        None => tracing::info!("{name}: {line}"),
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let mut instance = Self {
            descriptor: descriptor.clone(),
            stdin: child.stdin.take(),
            child: Some(child),
            responses,
            next: 0,
            crashed: Arc::new(AtomicBool::new(false)),
            reported: false,
            processor_alive: Arc::new(AtomicBool::new(false)),
            active: false,
            gui_open: false,
            last_state: None,
            texts: HashMap::new(),
        };
        match instance.call(Request::Load(descriptor.clone()), LOAD_TIMEOUT)? {
            Response::Done => Ok(instance),
            Response::Failed(e) => Err(e),
            other => Err(format!("unexpected answer {other:?}")),
        }
    }

    /// Process id of the host, for diagnostics and tests.
    pub fn pid(&self) -> u32 {
        self.child.as_ref().map_or(0, Child::id)
    }

    fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child
            .as_mut()
            .and_then(|c| c.try_wait().ok().flatten())
    }

    pub fn crashed(&self) -> bool {
        self.crashed.load(Ordering::Relaxed)
    }

    fn crash(&mut self, why: &str) {
        if !self.crashed.swap(true, Ordering::Relaxed) {
            tracing::warn!("plugin {} stopped: {why}", self.descriptor.name);
            if let Some(child) = &mut self.child {
                let _ = child.kill();
            }
            self.stdin = None;
        }
    }

    fn call(&mut self, request: Request, timeout: Duration) -> Result<Response, String> {
        if self.crashed() {
            return Err("the plugin crashed".into());
        }
        if let Some(status) = self.exited() {
            self.crash(&format!("host exited ({status})"));
            return Err("the plugin crashed".into());
        }
        self.next += 1;
        let id = self.next;
        let name = request.name();
        let line = serde_json::to_string(&(id, &request)).map_err(|e| e.to_string())?;
        let written = self
            .stdin
            .as_mut()
            .is_some_and(|s| writeln!(s, "{line}").and_then(|()| s.flush()).is_ok());
        if !written {
            self.crash("host closed its input");
            return Err("the plugin crashed".into());
        }
        loop {
            match self.responses.recv_timeout(timeout) {
                Ok((answer, response)) if answer == id => return Ok(response),
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.crash(&format!(
                        "not responding ({name} unanswered after {}s)",
                        timeout.as_secs()
                    ));
                    return Err("the plugin stopped responding".into());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.crash("host exited");
                    return Err("the plugin crashed".into());
                }
            }
        }
    }
}

/// Puts a plugin host in a job closed with AudioStrada, so that it cannot outlive it (Ctrl+C,
/// crash), as Windows does not end children with their parent.
#[cfg(windows)]
fn kill_with_audiostrada(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    /// The job's handle, never closed: the system closes it when AudioStrada exits.
    static JOB: OnceLock<Option<usize>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls with a valid, sized structure.
        unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            Some(job.0 as usize)
        }
    });
    if let Some(job) = *job {
        // SAFETY: the job lives as long as AudioStrada, the child's handle as long as `child`.
        let assigned = unsafe {
            AssignProcessToJobObject(
                HANDLE(job as *mut std::ffi::c_void),
                HANDLE(child.as_raw_handle()),
            )
        };
        if let Err(e) = assigned {
            tracing::warn!("plugin host not tied to AudioStrada: {e}");
        }
    }
}

impl Drop for SandboxInstance {
    fn drop(&mut self) {
        // Closing stdin asks the host to quit; a stuck host is killed.
        self.stdin = None;
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("plugin-host reaper".into())
            .spawn(move || {
                for _ in 0..100 {
                    if let Ok(Some(_)) = child.try_wait() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = child.kill();
                let _ = child.wait();
            });
    }
}

impl PluginInstance for SandboxInstance {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        max_frames: usize,
    ) -> Result<Box<dyn PluginProcessor>, String> {
        let layout = Layout::new(max_frames.max(1));
        let shared = Shared::create(&shm::unique_name(), layout.len)?;
        let region = Region::new(shared, layout)?;
        let request = Request::Activate {
            shm: region.shared.name().to_owned(),
            sample_rate,
            max_frames: layout.max_frames,
        };
        match self.call(request, LOAD_TIMEOUT)? {
            Response::Done => {}
            Response::Failed(e) => return Err(e),
            other => return Err(format!("unexpected answer {other:?}")),
        }
        self.active = true;
        self.processor_alive.store(true, Ordering::Relaxed);
        Ok(Box::new(SandboxProcessor {
            region,
            seq: 0,
            rate: sample_rate.max(1),
            offline: false,
            crashed: Arc::clone(&self.crashed),
            alive: Arc::clone(&self.processor_alive),
        }))
    }

    fn try_deactivate(&mut self) -> bool {
        if self.processor_alive.load(Ordering::Relaxed) {
            return false;
        }
        if self.active {
            self.active = false;
            let _ = self.call(Request::Deactivate, CALL_TIMEOUT);
        }
        true
    }

    fn params(&mut self) -> Vec<ParamDesc> {
        match self.call(Request::Params, CALL_TIMEOUT) {
            Ok(Response::Params(p)) => p,
            _ => Vec::new(),
        }
    }

    fn param_value(&mut self, id: u32) -> Option<f64> {
        match self.call(Request::ParamValue(id), CALL_TIMEOUT) {
            Ok(Response::Value(v)) => v,
            _ => None,
        }
    }

    fn value_text(&mut self, id: u32, value: f64) -> Option<String> {
        if let Some(text) = self.texts.get(&(id, value.to_bits())) {
            return text.clone();
        }
        let text = match self.call(Request::ValueText(id, value), CALL_TIMEOUT) {
            Ok(Response::Text(t)) => t,
            _ => return None,
        };
        if self.texts.len() > 4096 {
            self.texts.clear();
        }
        self.texts.insert((id, value.to_bits()), text.clone());
        text
    }

    fn set_param(&mut self, id: u32, value: f64) {
        let _ = self.call(Request::SetParam(id, value), CALL_TIMEOUT);
    }

    fn latency(&mut self) -> u32 {
        match self.call(Request::Latency, CALL_TIMEOUT) {
            Ok(Response::Latency(l)) => l,
            _ => 0,
        }
    }

    fn save_state(&mut self) -> Option<Vec<u8>> {
        if let Ok(Response::State(Some(state))) = self.call(Request::SaveState, CALL_TIMEOUT) {
            self.last_state = Some(state);
        }
        // After a crash, the last state saved lets the plugin come back where it was.
        self.last_state.clone()
    }

    fn load_state(&mut self, data: &[u8]) -> bool {
        let loaded = matches!(
            self.call(Request::LoadState(data.to_vec()), LOAD_TIMEOUT),
            Ok(Response::Flag(true))
        );
        if loaded {
            self.last_state = Some(data.to_vec());
        }
        loaded
    }

    fn poll(&mut self) -> Vec<PluginRequest> {
        if self.crashed() || self.exited().is_some() {
            self.crash("host exited");
            if !std::mem::replace(&mut self.reported, true) {
                self.gui_open = false;
                return vec![PluginRequest::Crashed];
            }
            return Vec::new();
        }
        match self.call(Request::Poll, CALL_TIMEOUT) {
            Ok(Response::Requests(requests)) => {
                if requests.contains(&PluginRequest::RescanParams) {
                    self.texts.clear();
                }
                if requests.contains(&PluginRequest::GuiClosed) {
                    self.gui_open = false;
                }
                requests
            }
            _ => Vec::new(),
        }
    }

    fn has_gui(&mut self) -> bool {
        matches!(
            self.call(Request::HasGui, CALL_TIMEOUT),
            Ok(Response::Flag(true))
        )
    }

    fn open_gui(&mut self, title: &str) -> Result<(), String> {
        let request = Request::OpenGui(
            title.to_owned(),
            crate::window::parent(),
            crate::window::grabbed_keys(),
        );
        match self.call(request, LOAD_TIMEOUT)? {
            Response::Done => {
                self.gui_open = true;
                Ok(())
            }
            Response::Failed(e) => Err(e),
            other => Err(format!("unexpected answer {other:?}")),
        }
    }

    fn close_gui(&mut self) {
        if self.gui_open {
            let _ = self.call(Request::CloseGui, CALL_TIMEOUT);
            self.gui_open = false;
        }
    }

    fn gui_open(&self) -> bool {
        self.gui_open
    }

    fn raise_gui(&mut self) {
        if self.gui_open {
            let _ = self.call(Request::RaiseGui, CALL_TIMEOUT);
        }
    }
}

/// Audio side of a sandboxed plugin: hands each block to the host and waits for it.
pub struct SandboxProcessor {
    region: Region,
    seq: u32,
    rate: u32,
    offline: bool,
    crashed: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

impl std::fmt::Debug for SandboxProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxProcessor").finish_non_exhaustive()
    }
}

impl SandboxProcessor {
    fn timeout(&self, frames: usize) -> Duration {
        if self.offline {
            return Duration::from_secs(5);
        }
        let block = Duration::from_secs_f64(frames as f64 / f64::from(self.rate));
        block.max(Duration::from_millis(2))
    }
}

impl PluginProcessor for SandboxProcessor {
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        params_out: &mut dyn FnMut(u32, f64),
    ) {
        if self.crashed.load(Ordering::Relaxed) {
            return;
        }
        let r = &self.region;
        let n = left.len().min(right.len()).min(r.layout.max_frames);
        let timeout = self.timeout(n);
        let header = r.header();
        // The host may still be busy with a block that came back too late.
        if header.done.load(Ordering::Acquire) != self.seq
            && !r.done.wait_while(self.seq.wrapping_sub(1), timeout)
        {
            return;
        }
        let l = &r.layout;
        // SAFETY: the host is idle (done == seq), so these buffers are ours.
        unsafe {
            r.slice::<f32>(l.left, n).copy_from_slice(&left[..n]);
            r.slice::<f32>(l.right, n).copy_from_slice(&right[..n]);
            let changes = r.slice::<Change>(l.params_in, MAX_CHANGES);
            let count = params_in.len().min(MAX_CHANGES);
            for (slot, &(id, value)) in changes.iter_mut().zip(&params_in[..count]) {
                *slot = Change { id, _pad: 0, value };
            }
            header.params_in.store(count as u32, Ordering::Relaxed);
            let events = r.slice::<Midi>(l.midi, MAX_MIDI);
            let count = midi.len().min(MAX_MIDI);
            for (slot, e) in events.iter_mut().zip(&midi[..count]) {
                *slot = Midi {
                    time: e.time,
                    data: [e.data[0], e.data[1], e.data[2], 0],
                };
            }
            header.midi.store(count as u32, Ordering::Relaxed);
        }
        header.frames.store(n as u32, Ordering::Relaxed);
        let previous = self.seq;
        self.seq = self.seq.wrapping_add(1);
        r.request.set(self.seq);
        if !r.done.wait_while(previous, timeout) || header.done.load(Ordering::Acquire) != self.seq
        {
            return;
        }
        // SAFETY: the host answered this block and is idle again.
        unsafe {
            left[..n].copy_from_slice(r.slice::<f32>(l.left, n));
            right[..n].copy_from_slice(r.slice::<f32>(l.right, n));
            let count = (header.params_out.load(Ordering::Relaxed) as usize).min(MAX_CHANGES);
            for c in &r.slice::<Change>(l.params_out, MAX_CHANGES)[..count] {
                params_out(c.id, c.value);
            }
        }
    }

    fn reset(&mut self) {}

    fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }
}

impl Drop for SandboxProcessor {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

/// The host side: an audio thread serving blocks from shared memory.
struct AudioThread {
    stop: Arc<AtomicBool>,
    region: Arc<Region>,
    handle: std::thread::JoinHandle<()>,
}

impl AudioThread {
    /// `period`: the duration of a block, for the thread's real-time scheduling.
    fn start(
        mut processor: Box<dyn PluginProcessor>,
        region: Region,
        period: Duration,
    ) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let region = Arc::new(region);
        let (s, r) = (Arc::clone(&stop), Arc::clone(&region));
        // Read before answering Activate: the engine may send a block right after.
        let mut last = region.header().request.load(Ordering::Acquire);
        let handle = std::thread::Builder::new()
            .name("plugin audio".into())
            .spawn(move || {
                let _ = audiostrada_rt::thread::promote_audio_thread(period);
                let mut changes = Vec::with_capacity(MAX_CHANGES);
                let mut events = Vec::with_capacity(MAX_MIDI);
                while !s.load(Ordering::Relaxed) {
                    if !r.request.wait_while(last, Duration::from_millis(100)) {
                        continue;
                    }
                    last = r.header().request.load(Ordering::Acquire);
                    serve(&r, processor.as_mut(), &mut changes, &mut events);
                    r.done.set(last);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stop,
            region,
            handle,
        })
    }

    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.region.request.wake();
        let _ = self.handle.join();
    }
}

fn serve(
    r: &Region,
    processor: &mut dyn PluginProcessor,
    changes: &mut Vec<(u32, f64)>,
    events: &mut Vec<MidiEvent>,
) {
    let header = r.header();
    let l = &r.layout;
    let n = (header.frames.load(Ordering::Relaxed) as usize).min(l.max_frames);
    // SAFETY: the engine handed this block over (request) and waits for `done`.
    unsafe {
        let count = (header.params_in.load(Ordering::Relaxed) as usize).min(MAX_CHANGES);
        changes.clear();
        changes.extend(
            r.slice::<Change>(l.params_in, MAX_CHANGES)[..count]
                .iter()
                .map(|c| (c.id, c.value)),
        );
        let count = (header.midi.load(Ordering::Relaxed) as usize).min(MAX_MIDI);
        events.clear();
        events.extend(
            r.slice::<Midi>(l.midi, MAX_MIDI)[..count]
                .iter()
                .map(|m| MidiEvent {
                    time: m.time,
                    data: [m.data[0], m.data[1], m.data[2]],
                }),
        );
        let left = r.slice::<f32>(l.left, n);
        let right = r.slice::<f32>(l.right, n);
        let out = r.slice::<Change>(l.params_out, MAX_CHANGES);
        let mut written = 0;
        processor.process(left, right, changes, events, &mut |id, value| {
            if written < MAX_CHANGES {
                out[written] = Change { id, _pad: 0, value };
                written += 1;
            }
        });
        header.params_out.store(written as u32, Ordering::Relaxed);
    }
}

/// Entry point of `audiostrada plugin-host`: serves one plugin until stdin closes.
pub fn run_host() -> i32 {
    let (tx, requests) = mpsc::channel::<(u64, Request)>();
    let waker = messages::Waker::new();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            match serde_json::from_str(&line) {
                Ok(r) => {
                    if tx.send(r).is_err() {
                        break;
                    }
                    waker.wake();
                }
                Err(e) => eprintln!("plugin host: bad request: {e}"),
            }
        }
        drop(tx);
        waker.wake();
    });
    let mut host = Host::default();
    loop {
        match messages::next_request(&requests, Duration::from_millis(10)) {
            Ok((id, request)) => {
                let response = host.handle(request);
                let line = serde_json::to_string(&(id, response)).unwrap_or_default();
                let mut out = std::io::stdout().lock();
                if writeln!(out, "{PREFIX}{line}")
                    .and_then(|()| out.flush())
                    .is_err()
                {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some(instance) = &mut host.instance {
            host.pending.extend(instance.poll());
        }
    }
    // AudioStrada is gone or done with us: a plugin stuck in its shutdown must not keep us alive.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        std::process::exit(0);
    });
    host.shutdown();
    0
}

/// Waiting for requests on the host's main thread, which also runs the plugin windows: on
/// Windows their messages are dispatched meanwhile, or they would neither draw nor answer.
mod messages {
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Duration;

    #[cfg(windows)]
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_NOREMOVE, PM_REMOVE, PeekMessageW,
            PostThreadMessageW, QS_ALLINPUT, TranslateMessage, WM_NULL,
        },
    };

    /// Wakes the main thread when a request arrives.
    #[cfg(not(windows))]
    pub struct Waker;

    #[cfg(not(windows))]
    impl Waker {
        pub fn new() -> Self {
            #[cfg(target_os = "macos")]
            cocoa::start();
            Self
        }

        pub fn wake(&self) {}
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    pub fn next_request<T>(
        requests: &mpsc::Receiver<T>,
        timeout: Duration,
    ) -> Result<T, RecvTimeoutError> {
        requests.recv_timeout(timeout)
    }

    /// Wakes the main thread when a request arrives, with a thread message.
    #[cfg(windows)]
    pub struct Waker(u32);

    #[cfg(windows)]
    impl Waker {
        /// On the main thread, whose message queue it creates.
        pub fn new() -> Self {
            let mut msg = MSG::default();
            // SAFETY: plain Win32 calls on the current thread.
            unsafe {
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                Self(GetCurrentThreadId())
            }
        }

        pub fn wake(&self) {
            // SAFETY: plain Win32 call; a lost message only delays the request to the timeout.
            unsafe {
                let _ = PostThreadMessageW(self.0, WM_NULL, WPARAM(0), LPARAM(0));
            }
        }
    }

    #[cfg(windows)]
    pub fn next_request<T>(
        requests: &mpsc::Receiver<T>,
        timeout: Duration,
    ) -> Result<T, RecvTimeoutError> {
        let take = || {
            requests.try_recv().map_err(|e| match e {
                mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        };
        dispatch();
        if let r @ (Ok(_) | Err(RecvTimeoutError::Disconnected)) = take() {
            return r;
        }
        let ms = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        // SAFETY: waits for a message on this thread's queue, no handles.
        unsafe {
            MsgWaitForMultipleObjects(None, false, ms, QS_ALLINPUT);
        }
        dispatch();
        take()
    }

    /// On macOS the plugin windows need AppKit's events: they are handled while waiting, and a
    /// request waits at most until the timeout.
    #[cfg(target_os = "macos")]
    pub fn next_request<T>(
        requests: &mpsc::Receiver<T>,
        timeout: Duration,
    ) -> Result<T, RecvTimeoutError> {
        let take = || {
            requests.try_recv().map_err(|e| match e {
                mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        };
        if let r @ (Ok(_) | Err(RecvTimeoutError::Disconnected)) = take() {
            return r;
        }
        if !cocoa::dispatch(timeout) {
            return requests.recv_timeout(timeout);
        }
        take()
    }

    #[cfg(target_os = "macos")]
    mod cocoa {
        use std::time::Duration;

        use objc2::MainThreadMarker;
        use objc2::rc::autoreleasepool;
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEventMask};
        use objc2_foundation::{NSDate, NSDefaultRunLoopMode};

        /// Makes the plugin host an AppKit application without a Dock icon.
        pub fn start() {
            if let Some(mtm) = MainThreadMarker::new() {
                let app = NSApplication::sharedApplication(mtm);
                app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
                app.finishLaunching();
            }
        }

        /// Handles AppKit events for up to `timeout`; false off the main thread.
        pub fn dispatch(timeout: Duration) -> bool {
            let Some(mtm) = MainThreadMarker::new() else {
                return false;
            };
            let app = NSApplication::sharedApplication(mtm);
            autoreleasepool(|_| {
                // SAFETY: an AppKit constant, valid for the life of the process.
                let mode = unsafe { NSDefaultRunLoopMode };
                let mut until = NSDate::dateWithTimeIntervalSinceNow(timeout.as_secs_f64());
                while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&until),
                    mode,
                    true,
                ) {
                    app.sendEvent(&event);
                    until = NSDate::distantPast();
                }
                app.updateWindows();
            });
            true
        }
    }

    #[cfg(windows)]
    fn dispatch() {
    /// Handles the windows' messages for a while, then hands back to the requests: a plugin
    /// that posts itself a message for each one it handles (or never validates its painting)
    /// keeps the queue full forever, and emptying it would never answer AudioStrada again.
        let mut msg = MSG::default();
        // SAFETY: standard message pumping on this thread's queue.
        const BUDGET: Duration = Duration::from_millis(50);
        let start = std::time::Instant::now();
        unsafe {
            while start.elapsed() < BUDGET
                && PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool()
            {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[derive(Default)]
struct Host {
    instance: Option<Box<dyn PluginInstance>>,
    audio: Option<AudioThread>,
    pending: Vec<PluginRequest>,
}

impl Host {
    fn handle(&mut self, request: Request) -> Response {
        if let Request::Load(descriptor) = &request {
            return match crate::instantiate_in_process(descriptor) {
                Ok(i) => {
                    self.instance = Some(i);
                    Response::Done
                }
                Err(e) => Response::Failed(e),
            };
        }
        if let Request::Deactivate = request {
            self.deactivate();
            return Response::Done;
        }
        let Some(instance) = self.instance.as_mut() else {
            return Response::Failed("no plugin loaded".into());
        };
        match request {
            Request::Load(_) | Request::Deactivate => Response::Done,
            Request::Params => Response::Params(instance.params()),
            Request::ParamValue(id) => Response::Value(instance.param_value(id)),
            Request::ValueText(id, v) => Response::Text(instance.value_text(id, v)),
            Request::SetParam(id, v) => {
                instance.set_param(id, v);
                Response::Done
            }
            Request::Latency => Response::Latency(instance.latency()),
            Request::SaveState => Response::State(instance.save_state()),
            Request::LoadState(data) => Response::Flag(instance.load_state(&data)),
            Request::Poll => {
                let mut requests = std::mem::take(&mut self.pending);
                requests.extend(instance.poll());
                Response::Requests(requests)
            }
            Request::HasGui => Response::Flag(instance.has_gui()),
            Request::OpenGui(title, parent, keys) => {
                crate::window::set_parent(parent);
                crate::window::set_grabbed_keys(keys);
                match instance.open_gui(&title) {
                    Ok(()) => Response::Done,
                    Err(e) => Response::Failed(e),
                }
            }
            Request::CloseGui => {
                instance.close_gui();
                Response::Done
            }
            Request::RaiseGui => {
                instance.raise_gui();
                Response::Done
            }
            Request::Activate {
                shm,
                sample_rate,
                max_frames,
            } => {
                let layout = Layout::new(max_frames);
                let region =
                    match Shared::open(&shm, layout.len).and_then(|s| Region::new(s, layout)) {
                        Ok(r) => r,
                        Err(e) => return Response::Failed(e),
                    };
                let processor = match instance.activate(sample_rate, max_frames) {
                    Ok(p) => p,
                    Err(e) => return Response::Failed(e),
                };
                let period =
                    Duration::from_secs_f64(max_frames as f64 / f64::from(sample_rate.max(1)));
                match AudioThread::start(processor, region, period) {
                    Ok(audio) => {
                        self.audio = Some(audio);
                        Response::Done
                    }
                    Err(e) => Response::Failed(e),
                }
            }
        }
    }

    fn deactivate(&mut self) {
        if let Some(audio) = self.audio.take() {
            audio.stop();
        }
        if let Some(instance) = &mut self.instance {
            instance.try_deactivate();
        }
    }

    fn shutdown(&mut self) {
        self.deactivate();
        if let Some(instance) = &mut self.instance {
            instance.close_gui();
        }
        self.instance = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_aligned_and_ordered() {
        let l = Layout::new(1000);
        assert!(size_of::<Header>() <= l.left);
        for offset in [l.left, l.right, l.params_in, l.midi, l.params_out, l.len] {
            assert_eq!(offset % 64, 0);
        }
        assert!(l.right >= l.left + 4000 && l.params_out < l.len);
    }

    /// Doubles the audio, answers each parameter on the next id and reports the MIDI count.
    #[derive(Debug)]
    struct Gain;

    impl PluginProcessor for Gain {
        fn process(
            &mut self,
            left: &mut [f32],
            right: &mut [f32],
            params_in: &[(u32, f64)],
            midi: &[MidiEvent],
            params_out: &mut dyn FnMut(u32, f64),
        ) {
            for s in left.iter_mut().chain(right.iter_mut()) {
                *s *= 2.0;
            }
            for &(id, value) in params_in {
                params_out(id + 1, value * 2.0);
            }
            params_out(99, midi.len() as f64);
        }

        fn reset(&mut self) {}
    }

    #[derive(Debug)]
    struct Fake {
        descriptor: PluginDescriptor,
        state: Vec<u8>,
    }

    impl PluginInstance for Fake {
        fn descriptor(&self) -> &PluginDescriptor {
            &self.descriptor
        }
        fn activate(&mut self, _: u32, _: usize) -> Result<Box<dyn PluginProcessor>, String> {
            Ok(Box::new(Gain))
        }
        fn try_deactivate(&mut self) -> bool {
            true
        }
        fn params(&mut self) -> Vec<ParamDesc> {
            Vec::new()
        }
        fn param_value(&mut self, id: u32) -> Option<f64> {
            Some(f64::from(id))
        }
        fn value_text(&mut self, id: u32, value: f64) -> Option<String> {
            Some(format!("{id}={value}"))
        }
        fn latency(&mut self) -> u32 {
            7
        }
        fn save_state(&mut self) -> Option<Vec<u8>> {
            Some(self.state.clone())
        }
        fn load_state(&mut self, data: &[u8]) -> bool {
            self.state = data.to_vec();
            true
        }
        fn poll(&mut self) -> Vec<PluginRequest> {
            vec![PluginRequest::RescanParams]
        }
        fn has_gui(&mut self) -> bool {
            false
        }
        fn open_gui(&mut self, _: &str) -> Result<(), String> {
            Err("no window".into())
        }
        fn close_gui(&mut self) {}
        fn gui_open(&self) -> bool {
            false
        }
    }

    fn descriptor(format: crate::Format) -> PluginDescriptor {
        PluginDescriptor {
            format,
            id: "fake".into(),
            name: "Fake".into(),
            vendor: String::new(),
            version: String::new(),
            path: std::path::PathBuf::new(),
            instrument: false,
            tags: Vec::new(),
        }
    }

    /// The engine's side of a region created for `max_frames`, and its name.
    fn engine_side(max_frames: usize) -> (SandboxProcessor, String, Arc<AtomicBool>) {
        let layout = Layout::new(max_frames);
        let region = Region::new(
            Shared::create(&shm::unique_name(), layout.len).unwrap(),
            layout,
        )
        .unwrap();
        let name = region.shared.name().to_owned();
        let alive = Arc::new(AtomicBool::new(true));
        let processor = SandboxProcessor {
            region,
            seq: 0,
            rate: 48_000,
            offline: true,
            crashed: Arc::new(AtomicBool::new(false)),
            alive: Arc::clone(&alive),
        };
        (processor, name, alive)
    }

    #[test]
    fn host_serves_requests_and_blocks() {
        let mut host = Host::default();
        assert!(matches!(
            host.handle(Request::Latency),
            Response::Failed(e) if e == "no plugin loaded"
        ));
        assert!(matches!(
            host.handle(Request::Load(descriptor(crate::Format::Au))),
            Response::Failed(_)
        ));
        host.instance = Some(Box::new(Fake {
            descriptor: descriptor(crate::Format::Clap),
            state: Vec::new(),
        }));
        host.pending.push(PluginRequest::Restart);
        assert!(matches!(
            host.handle(Request::Latency),
            Response::Latency(7)
        ));
        assert!(matches!(
            host.handle(Request::ParamValue(3)),
            Response::Value(Some(3.0))
        ));
        assert!(matches!(
            host.handle(Request::ValueText(1, 0.5)),
            Response::Text(Some(t)) if t == "1=0.5"
        ));
        assert!(matches!(
            host.handle(Request::LoadState(vec![1, 2])),
            Response::Flag(true)
        ));
        assert!(matches!(
            host.handle(Request::SaveState),
            Response::State(Some(s)) if s == [1, 2]
        ));
        assert!(matches!(
            host.handle(Request::Poll),
            Response::Requests(r) if r == [PluginRequest::Restart, PluginRequest::RescanParams]
        ));
        assert!(matches!(
            host.handle(Request::HasGui),
            Response::Flag(false)
        ));
        assert!(matches!(
            host.handle(Request::OpenGui("t".into(), 0, Vec::new())),
            Response::Failed(e) if e == "no window"
        ));
        for request in [
            Request::SetParam(1, 0.0),
            Request::CloseGui,
            Request::RaiseGui,
        ] {
            assert!(matches!(host.handle(request), Response::Done));
        }
        assert!(matches!(host.handle(Request::Params), Response::Params(p) if p.is_empty()));

        let (mut processor, shm, alive) = engine_side(64);
        let activate = Request::Activate {
            shm,
            sample_rate: 48_000,
            max_frames: 64,
        };
        assert!(matches!(host.handle(activate), Response::Done));
        for block in 0..3 {
            let (mut left, mut right) = (vec![0.25f32; 32], vec![0.5f32; 32]);
            let mut out = Vec::new();
            let notes = [MidiEvent::note_on(0, 0, 60, 100); 2];
            processor.process(
                &mut left,
                &mut right,
                &[(4, 0.5)],
                &notes[..block],
                &mut |id, v| out.push((id, v)),
            );
            assert_eq!((left[31], right[0]), (0.5, 1.0), "block {block}");
            assert_eq!(out, vec![(5, 1.0), (99, block as f64)]);
        }
        assert!(matches!(host.handle(Request::Deactivate), Response::Done));
        drop(processor);
        assert!(!alive.load(Ordering::Relaxed));
        host.shutdown();
        assert!(host.instance.is_none());
    }

    #[test]
    fn crashed_or_missing_hosts_leave_audio_untouched() {
        let (mut processor, _, _) = engine_side(16);
        assert_eq!(processor.timeout(480), Duration::from_secs(5));
        processor.set_offline(false);
        assert_eq!(processor.timeout(480), Duration::from_millis(10));
        assert_eq!(processor.timeout(1), Duration::from_millis(2));
        processor.crashed.store(true, Ordering::Relaxed);
        let (mut left, mut right) = ([1.0f32; 8], [1.0f32; 8]);
        processor.process(&mut left, &mut right, &[], &[], &mut |_, _| unreachable!());
        assert_eq!(left, [1.0; 8]);
        assert!(Shared::open(&shm::unique_name(), 64).is_err());
        let error = SandboxInstance::new(
            Path::new("/nonexistent/audiostrada"),
            &descriptor(crate::Format::Clap),
        )
        .unwrap_err();
        assert!(error.contains("cannot start"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_host_that_exits_is_reported() {
        let error =
            SandboxInstance::new(Path::new("true"), &descriptor(crate::Format::Clap)).unwrap_err();
        assert!(error.contains("crashed"), "{error}");
    }

    #[test]
    fn signals_wake_across_mappings() {
        let name = shm::unique_name();
        let a = Shared::create(&name, 4096).unwrap();
        let b = Shared::open(&name, 4096).unwrap();
        let (sa, sb) = (a.signal(0, "s").unwrap(), b.signal(0, "s").unwrap());
        assert!(!sb.wait_while(0, Duration::from_millis(5)));
        let waiter = std::thread::spawn(move || sb.wait_while(0, Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(20));
        sa.set(7);
        assert!(waiter.join().unwrap());
    }
}
