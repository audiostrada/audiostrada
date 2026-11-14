//! Multi-core execution of a schedule.
//!
//! Each cycle, the audio thread runs the hardware input node, then processes the DAG of
//! processor nodes together with the workers: a node becomes ready when all its sources are
//! done (atomic counters) and is pushed to a lock-free queue. The audio thread waits until every
//! worker has left the cycle before returning, so the schedule can then be swapped safely.
//!
//! With an automatic worker count, a light graph runs on the audio thread alone: the time its
//! nodes take is measured every cycle, and the workers only join when it gets heavy.

use std::cell::UnsafeCell;
use std::hint::spin_loop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle, Thread};

use audiostrada_io::AudioBuffers;

use crate::TransportInfo;
use crate::graph::NodeKind;
use crate::schedule::{Schedule, ScheduledNode};

/// Below this many processor nodes, waking workers costs more than it saves.
const PARALLEL_THRESHOLD: usize = 4;
const SPIN_BEFORE_PARK: u32 = 2_000;
/// Share of a block's duration the nodes may take before the workers join, and below which
/// they leave again.
const JOIN_LOAD: f64 = 0.4;
const LEAVE_LOAD: f64 = 0.2;

/// Everything a worker needs for the current cycle.
#[derive(Clone, Copy)]
struct Cycle {
    schedule: *mut Schedule,
    frames: usize,
    transport: TransportInfo,
}

struct Shared {
    cycle: UnsafeCell<Cycle>,
    generation: AtomicU64,
    active: AtomicBool,
    busy: AtomicUsize,
    remaining: AtomicUsize,
    stop: AtomicBool,
    /// Time spent in the nodes of the current cycle, all threads together (ns).
    spent: AtomicU64,
}

// SAFETY: `cycle` is written by the audio thread only while no worker is inside a cycle
// (`active` false and `busy` zero), and read by workers only while `active` is true.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

pub(crate) struct WorkerPool {
    shared: Arc<Shared>,
    threads: Vec<Thread>,
    handles: Vec<JoinHandle<()>>,
    /// The workers join only when the graph is heavy (else always).
    adaptive: bool,
    parallel: bool,
}

impl std::fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerPool")
            .field("workers", &self.threads.len())
            .finish()
    }
}

impl WorkerPool {
    /// `period`: the duration of a device block, for the workers' real-time scheduling. Only
    /// the adaptive pool of a live engine runs in real time: a render (export, tests) taking
    /// every core at a real-time priority would starve the whole desktop.
    pub fn new(workers: usize, adaptive: bool, period: std::time::Duration) -> Self {
        let shared = Arc::new(Shared {
            cycle: UnsafeCell::new(Cycle {
                schedule: std::ptr::null_mut(),
                frames: 0,
                transport: TransportInfo {
                    counting: false,
                    playing: false,
                    position: 0,
                    sample_rate: 0,
                    beat: 0.0,
                    bpm: 0.0,
                    bar: Default::default(),
                },
            }),
            generation: AtomicU64::new(0),
            active: AtomicBool::new(false),
            busy: AtomicUsize::new(0),
            remaining: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            spent: AtomicU64::new(0),
        });
        let mut handles = Vec::with_capacity(workers);
        for index in 0..workers {
            let shared = Arc::clone(&shared);
            match thread::Builder::new()
                .name(format!("audiostrada-worker-{index}"))
                .spawn(move || worker(&shared, period, adaptive))
            {
                Ok(handle) => handles.push(handle),
                Err(e) => tracing::warn!("cannot start engine worker: {e}"),
            }
        }
        Self {
            threads: handles.iter().map(|h| h.thread().clone()).collect(),
            shared,
            handles,
            adaptive,
            parallel: !adaptive,
        }
    }

    /// The workers take part in the cycles.
    pub fn parallel(&self) -> bool {
        self.parallel && !self.threads.is_empty()
    }

    /// Processes `frames` frames of the device block starting at `offset`.
    pub fn run(
        &mut self,
        schedule: &mut Schedule,
        io: &mut AudioBuffers<'_>,
        offset: usize,
        frames: usize,
        transport: &TransportInfo,
    ) {
        let hw_in = schedule.hardware_input;
        let hw_out = schedule.hardware_output;
        hardware_input(&mut schedule.nodes[hw_in], io, offset, frames);
        let started = std::time::Instant::now();
        let parallel = self.parallel
            && !self.threads.is_empty()
            && schedule.processor_count >= PARALLEL_THRESHOLD;
        let spent = if parallel {
            self.shared.spent.store(0, Ordering::Relaxed);
            self.run_parallel(schedule, frames, transport);
            self.shared.spent.load(Ordering::Relaxed)
        } else {
            for node in schedule
                .nodes
                .iter_mut()
                .filter(|n| n.kind == NodeKind::Processor)
            {
                node.run(frames, transport);
            }
            started.elapsed().as_nanos() as u64
        };
        if self.adaptive && transport.sample_rate > 0 {
            let budget = frames as f64 / f64::from(transport.sample_rate) * 1e9;
            let load = spent as f64 / budget;
            self.parallel = if self.parallel {
                load > LEAVE_LOAD
            } else {
                load > JOIN_LOAD
            };
        }
        hardware_output(&mut schedule.nodes[hw_out], io, offset, frames);
    }

    fn run_parallel(&mut self, schedule: &mut Schedule, frames: usize, transport: &TransportInfo) {
        for (i, node) in schedule.nodes.iter().enumerate() {
            schedule.pending[i].store(schedule.deps[i], Ordering::Relaxed);
            if node.kind == NodeKind::Processor && schedule.deps[i] == 0 {
                let _ = schedule.ready.push(i);
            }
        }
        let shared = &*self.shared;
        shared
            .remaining
            .store(schedule.processor_count, Ordering::Relaxed);
        // SAFETY: no worker is inside a cycle (see `finish`), so nobody reads `cycle` now.
        unsafe {
            *shared.cycle.get() = Cycle {
                schedule,
                frames,
                transport: *transport,
            };
        }
        shared.active.store(true, Ordering::SeqCst);
        shared.generation.fetch_add(1, Ordering::SeqCst);
        for thread in &self.threads {
            thread.unpark();
        }
        // SAFETY: the cycle was just published and stays valid until `finish` returns.
        unsafe { work(shared) };
        shared.active.store(false, Ordering::SeqCst);
        while shared.busy.load(Ordering::SeqCst) != 0 {
            spin_loop();
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        for thread in &self.threads {
            thread.unpark();
        }
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

fn worker(shared: &Shared, period: std::time::Duration, realtime: bool) {
    if realtime && let Err(e) = audiostrada_rt::thread::promote_audio_thread(period) {
        tracing::debug!("engine worker without real-time priority: {e}");
    }
    let mut seen = 0;
    let mut idle = 0u32;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let generation = shared.generation.load(Ordering::SeqCst);
        if generation == seen {
            if idle < SPIN_BEFORE_PARK {
                idle += 1;
                spin_loop();
            } else {
                thread::park();
            }
            continue;
        }
        seen = generation;
        idle = 0;
        shared.busy.fetch_add(1, Ordering::SeqCst);
        if shared.active.load(Ordering::SeqCst) {
            // SAFETY: `active` was seen after registering in `busy`, so the audio thread waits
            // for us before touching the cycle again.
            unsafe { work(shared) };
        }
        shared.busy.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Runs ready nodes until the whole DAG is done.
///
/// # Safety
/// The published cycle must be valid for the duration of the call.
unsafe fn work(shared: &Shared) {
    // SAFETY: guaranteed by the caller.
    let cycle = unsafe { *shared.cycle.get() };
    let schedule = cycle.schedule;
    // SAFETY: the schedule outlives the cycle; only atomics and the queue are shared.
    let (ready, pending, nodes) = unsafe {
        (
            &(*schedule).ready,
            &(*schedule).pending,
            (*schedule).nodes.as_mut_ptr(),
        )
    };
    while shared.remaining.load(Ordering::Acquire) > 0 {
        let Some(index) = ready.pop() else {
            spin_loop();
            continue;
        };
        // SAFETY: each index is popped exactly once per cycle, giving exclusive access to the node;
        // all of its sources completed before it was pushed (AcqRel on `pending`).
        let node: &mut ScheduledNode = unsafe { &mut *nodes.add(index) };
        let started = std::time::Instant::now();
        node.run(cycle.frames, &cycle.transport);
        shared
            .spent
            .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        for &dependent in &node.dependents {
            if pending[dependent].fetch_sub(1, Ordering::AcqRel) == 1 {
                let _ = ready.push(dependent);
            }
        }
        shared.remaining.fetch_sub(1, Ordering::AcqRel);
    }
}

fn hardware_input(node: &mut ScheduledNode, io: &AudioBuffers<'_>, offset: usize, frames: usize) {
    for (ch, &ptr) in node.out_ptrs.iter().enumerate() {
        // SAFETY: hardware input buffers belong to the schedule and hold `max_frames` samples.
        let dst = unsafe { std::slice::from_raw_parts_mut(ptr, frames) };
        if ch < io.input_channels() {
            dst.copy_from_slice(&io.input(ch)[offset..offset + frames]);
        } else {
            dst.fill(0.0);
        }
    }
}

fn hardware_output(
    node: &mut ScheduledNode,
    io: &mut AudioBuffers<'_>,
    offset: usize,
    frames: usize,
) {
    node.gather(frames);
    for (ch, &ptr) in node.in_ptrs.iter().enumerate().take(io.output_channels()) {
        // SAFETY: gathered input buffers hold `frames` valid samples.
        let src = unsafe { std::slice::from_raw_parts(ptr, frames) };
        io.output(ch)[offset..offset + frames].copy_from_slice(src);
    }
}
