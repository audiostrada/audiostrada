//! Compiled, real-time ready form of the graph: processing order, preallocated buffers,
//! plugin delay compensation and dependency counts for parallel execution.

use std::collections::HashMap;
use std::sync::atomic::AtomicU32;

use audiostrada_io::AudioBuffers;
use crossbeam_queue::ArrayQueue;

use crate::graph::{Graph, NodeId, NodeKind};
use crate::{ProcessContext, Processor, TransportInfo};

/// Fixed delay applied to one connection so that parallel paths stay aligned.
pub(crate) struct DelayLine {
    ring: Box<[f32]>,
    pos: usize,
}

impl DelayLine {
    fn new(delay: usize) -> Self {
        Self {
            ring: vec![0.0; delay].into_boxed_slice(),
            pos: 0,
        }
    }

    /// Adds `input` delayed by the line length to `output`.
    #[inline]
    fn add_delayed(&mut self, input: &[f32], output: &mut [f32]) {
        let len = self.ring.len();
        for (out, &sample) in output.iter_mut().zip(input) {
            *out += self.ring[self.pos];
            self.ring[self.pos] = sample;
            self.pos += 1;
            if self.pos == len {
                self.pos = 0;
            }
        }
    }

    fn clear(&mut self) {
        self.ring.fill(0.0);
    }
}

pub(crate) struct Source {
    buffer: *const f32,
    delay: Option<DelayLine>,
}

pub(crate) struct InputChannel {
    buffer: *mut f32,
    sources: Vec<Source>,
}

pub(crate) struct ScheduledNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub processor: Option<Box<dyn Processor>>,
    /// Index of this node in the schedule it replaces, to take its processor over.
    pub previous: Option<usize>,
    inputs: Vec<InputChannel>,
    pub in_ptrs: Vec<*const f32>,
    pub out_ptrs: Vec<*mut f32>,
    /// Processor nodes fed by this node (hardware nodes run outside the parallel pass).
    pub dependents: Vec<usize>,
}

impl ScheduledNode {
    /// Sums the sources of every input channel into the node's input buffers.
    #[inline]
    pub fn gather(&mut self, frames: usize) {
        for input in &mut self.inputs {
            // SAFETY: input buffers are owned by the schedule and hold `max_frames` samples;
            // sources were fully written by nodes that run before this one.
            let out = unsafe { std::slice::from_raw_parts_mut(input.buffer, frames) };
            match input.sources.as_mut_slice() {
                [] => out.fill(0.0),
                [
                    Source {
                        buffer,
                        delay: None,
                    },
                ] => {
                    out.copy_from_slice(unsafe { std::slice::from_raw_parts(*buffer, frames) });
                }
                sources => {
                    out.fill(0.0);
                    for source in sources {
                        let src = unsafe { std::slice::from_raw_parts(source.buffer, frames) };
                        match &mut source.delay {
                            Some(delay) => delay.add_delayed(src, out),
                            None => out.iter_mut().zip(src).for_each(|(o, s)| *o += s),
                        }
                    }
                }
            }
        }
    }

    /// Gathers inputs and runs the processor for one block.
    #[inline]
    pub fn run(&mut self, frames: usize, transport: &TransportInfo) {
        self.gather(frames);
        if let Some(processor) = &mut self.processor {
            // SAFETY: pointers target this node's own buffers, valid for `frames` samples.
            let buffers = unsafe { AudioBuffers::from_raw(&self.in_ptrs, &self.out_ptrs, frames) };
            processor.process(&mut ProcessContext { buffers, transport });
        }
    }

    pub fn reset(&mut self) {
        for source in self.inputs.iter_mut().flat_map(|i| &mut i.sources) {
            if let Some(delay) = &mut source.delay {
                delay.clear();
            }
        }
        if let Some(processor) = &mut self.processor {
            processor.reset();
        }
    }
}

pub(crate) struct Schedule {
    pub nodes: Vec<ScheduledNode>,
    pub hardware_input: usize,
    pub hardware_output: usize,
    /// Latency added by the graph between its inputs and the hardware output, in samples.
    pub latency: u32,
    pub max_frames: usize,
    /// Per node: number of processor nodes it waits for in the parallel pass.
    pub deps: Box<[u32]>,
    pub pending: Box<[AtomicU32]>,
    pub ready: ArrayQueue<usize>,
    pub processor_count: usize,
    _pool: Vec<Box<[f32]>>,
}

// SAFETY: raw pointers only reference buffers owned by `_pool`, which moves with the schedule.
unsafe impl Send for Schedule {}

impl std::fmt::Debug for Schedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Schedule")
            .field("nodes", &self.nodes.len())
            .field("latency", &self.latency)
            .finish()
    }
}

impl Schedule {
    /// Moves the processors of nodes kept from `old` into this schedule. Real-time safe.
    pub fn adopt(&mut self, old: &mut Schedule) {
        for node in &mut self.nodes {
            if let Some(previous) = node.previous.take()
                && let Some(old_node) = old.nodes.get_mut(previous)
            {
                node.processor = old_node.processor.take();
            }
        }
    }

    /// Takes every processor out, keyed by node, to hand them back to the graph.
    pub fn take_processors(&mut self) -> Vec<(NodeId, Box<dyn Processor>)> {
        self.nodes
            .iter_mut()
            .filter_map(|n| n.processor.take().map(|p| (n.id, p)))
            .collect()
    }
}

/// Compiles the graph. Processors still owned by the graph move into the schedule; nodes
/// already running on the audio thread (in `running`) are taken over from the previous schedule.
pub(crate) fn compile(
    graph: &mut Graph,
    running: &HashMap<NodeId, usize>,
    max_frames: usize,
) -> Schedule {
    let order = graph.topological_order();
    let index: HashMap<NodeId, usize> = order.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut pool: Vec<Box<[f32]>> = Vec::new();
    let alloc = |pool: &mut Vec<Box<[f32]>>| {
        let mut buffer = vec![0.0; max_frames].into_boxed_slice();
        let ptr = buffer.as_mut_ptr();
        pool.push(buffer);
        ptr
    };

    // Output buffers first, so that edges can point at them.
    let out_ptrs: Vec<Vec<*mut f32>> = order
        .iter()
        .map(|id| {
            (0..graph.nodes[id].outputs)
                .map(|_| alloc(&mut pool))
                .collect()
        })
        .collect();

    // Plugin delay compensation: arrival time of each node's input, then of its output.
    let mut input_time = vec![0u32; order.len()];
    let mut output_time = vec![0u32; order.len()];
    for (i, id) in order.iter().enumerate() {
        input_time[i] = graph
            .edges()
            .iter()
            .filter(|e| e.to.node == *id)
            .map(|e| output_time[index[&e.from.node]])
            .max()
            .unwrap_or(0);
        output_time[i] = input_time[i] + graph.nodes[id].latency;
    }

    let mut nodes = Vec::with_capacity(order.len());
    let mut deps = Vec::with_capacity(order.len());
    for (i, id) in order.iter().enumerate() {
        let processor = graph.nodes.get_mut(id).and_then(|s| s.processor.take());
        let spec = &graph.nodes[id];
        let inputs: Vec<InputChannel> = (0..spec.inputs)
            .map(|channel| InputChannel {
                buffer: alloc(&mut pool),
                sources: graph_sources(graph.edges(), *id, channel)
                    .map(|(src, src_channel)| {
                        let s = index[&src];
                        let delay = (input_time[i] - output_time[s]) as usize;
                        Source {
                            buffer: out_ptrs[s][src_channel].cast_const(),
                            delay: (delay > 0).then(|| DelayLine::new(delay)),
                        }
                    })
                    .collect(),
            })
            .collect();
        let in_ptrs = inputs.iter().map(|c| c.buffer.cast_const()).collect();
        let kind = spec.kind;
        let previous = if processor.is_none() {
            running.get(id).copied()
        } else {
            None
        };
        let is_processor = |n: NodeId| graph.nodes[&n].kind == NodeKind::Processor;
        let mut dependents: Vec<usize> = graph
            .edges()
            .iter()
            .filter(|e| e.from.node == *id && is_processor(e.to.node))
            .map(|e| index[&e.to.node])
            .collect();
        dependents.sort_unstable();
        dependents.dedup();
        let mut sources: Vec<usize> = graph
            .edges()
            .iter()
            .filter(|e| e.to.node == *id && is_processor(e.from.node))
            .map(|e| index[&e.from.node])
            .collect();
        sources.sort_unstable();
        sources.dedup();
        deps.push(sources.len() as u32);
        nodes.push(ScheduledNode {
            id: *id,
            kind,
            processor,
            previous,
            inputs,
            in_ptrs,
            out_ptrs: out_ptrs[i].clone(),
            dependents,
        });
    }
    let hardware_input = index[&graph.hardware_input()];
    let hardware_output = index[&graph.hardware_output()];
    let processor_count = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Processor)
        .count();
    Schedule {
        latency: input_time[hardware_output],
        pending: deps.iter().map(|_| AtomicU32::new(0)).collect(),
        deps: deps.into_boxed_slice(),
        ready: ArrayQueue::new(processor_count.max(1)),
        processor_count,
        nodes,
        hardware_input,
        hardware_output,
        max_frames,
        _pool: pool,
    }
}

fn graph_sources(
    edges: &[crate::Edge],
    node: NodeId,
    channel: usize,
) -> impl Iterator<Item = (NodeId, usize)> + '_ {
    edges
        .iter()
        .filter(move |e| e.to.node == node && e.to.channel == channel)
        .map(|e| (e.from.node, e.from.channel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_line_delays_across_blocks() {
        let mut line = DelayLine::new(3);
        let mut out = [0.0; 4];
        line.add_delayed(&[1.0, 2.0, 3.0, 4.0], &mut out);
        assert_eq!(out, [0.0, 0.0, 0.0, 1.0]);
        let mut out = [0.0; 2];
        line.add_delayed(&[5.0, 6.0], &mut out);
        assert_eq!(out, [2.0, 3.0]);
    }
}
