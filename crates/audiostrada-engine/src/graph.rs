//! Editable audio graph topology, owned by the non real-time side.
//!
//! Connections are made between individual channels, so any node can feed any other one
//! (Reaper-style free routing). The graph is always kept acyclic.

use std::collections::BTreeMap;

use crate::Processor;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Processor,
    /// Device inputs, exposed as output channels.
    HardwareInput,
    /// Device outputs, fed through input channels.
    HardwareOutput,
}

/// One channel of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Port {
    pub node: NodeId,
    pub channel: usize,
}

impl Port {
    pub fn new(node: NodeId, channel: usize) -> Self {
        Self { node, channel }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
    pub from: Port,
    pub to: Port,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    #[error("unknown node {0:?}")]
    UnknownNode(NodeId),
    #[error("node {node:?} has no channel {channel}")]
    InvalidChannel { node: NodeId, channel: usize },
    #[error("connection would create a cycle")]
    Cycle,
    #[error("connection already exists")]
    DuplicateEdge,
    #[error("hardware nodes cannot be removed")]
    HardwareNode,
}

pub(crate) struct NodeSpec {
    pub name: String,
    pub kind: NodeKind,
    pub inputs: usize,
    pub outputs: usize,
    pub latency: u32,
    /// Carries what is played live: it never waits for slower paths.
    pub live: bool,
    /// Its output is never aligned with the other paths (an input monitor).
    pub unaligned: bool,
    /// Present while the processor lives on this side (not yet sent to the audio thread).
    pub processor: Option<Box<dyn Processor>>,
}

/// Channels of the hardware nodes. Channels the device lacks read as silence or are dropped,
/// so routing survives device changes.
pub const HARDWARE_CHANNELS: usize = 64;

pub struct Graph {
    pub(crate) nodes: BTreeMap<NodeId, NodeSpec>,
    edges: Vec<Edge>,
    next_id: u64,
    hardware_input: NodeId,
    hardware_output: NodeId,
}

impl std::fmt::Debug for Graph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graph")
            .field("nodes", &self.nodes.len())
            .field("edges", &self.edges)
            .finish()
    }
}

impl Graph {
    pub(crate) fn new() -> Self {
        let mut graph = Self {
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            next_id: 0,
            hardware_input: NodeId(0),
            hardware_output: NodeId(0),
        };
        graph.hardware_input = graph.insert(
            "Hardware input",
            NodeKind::HardwareInput,
            0,
            HARDWARE_CHANNELS,
            None,
        );
        graph.hardware_output = graph.insert(
            "Hardware output",
            NodeKind::HardwareOutput,
            HARDWARE_CHANNELS,
            0,
            None,
        );
        graph
    }

    fn insert(
        &mut self,
        name: &str,
        kind: NodeKind,
        inputs: usize,
        outputs: usize,
        processor: Option<Box<dyn Processor>>,
    ) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.insert(
            id,
            NodeSpec {
                name: name.to_owned(),
                kind,
                inputs,
                outputs,
                latency: 0,
                live: false,
                unaligned: false,
                processor,
            },
        );
        id
    }

    pub fn hardware_input(&self) -> NodeId {
        self.hardware_input
    }

    pub fn hardware_output(&self) -> NodeId {
        self.hardware_output
    }

    pub(crate) fn add(
        &mut self,
        name: &str,
        processor: Box<dyn Processor>,
        inputs: usize,
        outputs: usize,
    ) -> NodeId {
        self.insert(name, NodeKind::Processor, inputs, outputs, Some(processor))
    }

    pub(crate) fn remove(&mut self, id: NodeId) -> Result<(), GraphError> {
        match self.nodes.get(&id).map(|n| n.kind) {
            None => Err(GraphError::UnknownNode(id)),
            Some(NodeKind::Processor) => {
                self.nodes.remove(&id);
                self.edges.retain(|e| e.from.node != id && e.to.node != id);
                Ok(())
            }
            Some(_) => Err(GraphError::HardwareNode),
        }
    }

    pub(crate) fn connect(&mut self, from: Port, to: Port) -> Result<(), GraphError> {
        let source = self
            .nodes
            .get(&from.node)
            .ok_or(GraphError::UnknownNode(from.node))?;
        if from.channel >= source.outputs {
            return Err(GraphError::InvalidChannel {
                node: from.node,
                channel: from.channel,
            });
        }
        let target = self
            .nodes
            .get(&to.node)
            .ok_or(GraphError::UnknownNode(to.node))?;
        if to.channel >= target.inputs {
            return Err(GraphError::InvalidChannel {
                node: to.node,
                channel: to.channel,
            });
        }
        let edge = Edge { from, to };
        if self.edges.contains(&edge) {
            return Err(GraphError::DuplicateEdge);
        }
        if from.node == to.node || self.reaches(to.node, from.node) {
            return Err(GraphError::Cycle);
        }
        self.edges.push(edge);
        Ok(())
    }

    pub(crate) fn disconnect(&mut self, from: Port, to: Port) -> bool {
        let before = self.edges.len();
        self.edges.retain(|e| *e != Edge { from, to });
        self.edges.len() != before
    }

    pub(crate) fn set_latency(&mut self, id: NodeId, latency: u32) -> Result<(), GraphError> {
        let node = self.nodes.get_mut(&id).ok_or(GraphError::UnknownNode(id))?;
        node.latency = latency;
        Ok(())
    }

    /// Returns true if it changed.
    pub(crate) fn set_live(&mut self, id: NodeId, live: bool) -> bool {
        self.nodes
            .get_mut(&id)
            .is_some_and(|n| std::mem::replace(&mut n.live, live) != live)
    }

    pub(crate) fn set_unaligned(&mut self, id: NodeId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.unaligned = true;
        }
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn name(&self, id: NodeId) -> Option<&str> {
        self.nodes.get(&id).map(|n| n.name.as_str())
    }

    /// True if `target` can be reached from `start` by following edges.
    fn reaches(&self, start: NodeId, target: NodeId) -> bool {
        let mut stack = vec![start];
        let mut seen = vec![start];
        while let Some(node) = stack.pop() {
            if node == target {
                return true;
            }
            for e in self.edges.iter().filter(|e| e.from.node == node) {
                if !seen.contains(&e.to.node) {
                    seen.push(e.to.node);
                    stack.push(e.to.node);
                }
            }
        }
        false
    }

    /// Nodes ordered so that every node comes after all of its sources (Kahn's algorithm).
    pub(crate) fn topological_order(&self) -> Vec<NodeId> {
        let mut in_degree: BTreeMap<NodeId, usize> = self.nodes.keys().map(|&id| (id, 0)).collect();
        let mut pairs: Vec<(NodeId, NodeId)> = self
            .edges
            .iter()
            .map(|e| (e.from.node, e.to.node))
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        for &(_, to) in &pairs {
            *in_degree.entry(to).or_default() += 1;
        }
        let mut ready: Vec<NodeId> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(&id, _)| id)
            .collect();
        ready.reverse();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(node) = ready.pop() {
            order.push(node);
            for &(_, to) in pairs.iter().filter(|(from, _)| *from == node) {
                let d = in_degree.entry(to).or_default();
                *d -= 1;
                if *d == 0 {
                    ready.push(to);
                }
            }
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProcessContext;

    fn silent() -> Box<dyn Processor> {
        Box::new(|_: &mut ProcessContext<'_>| {})
    }

    #[test]
    fn connections_are_validated() {
        let mut g = Graph::new();
        let a = g.add("a", silent(), 2, 2);
        let b = g.add("b", silent(), 2, 2);
        assert_eq!(g.connect(Port::new(a, 0), Port::new(b, 0)), Ok(()));
        assert_eq!(
            g.connect(Port::new(a, 0), Port::new(b, 0)),
            Err(GraphError::DuplicateEdge)
        );
        assert_eq!(
            g.connect(Port::new(b, 0), Port::new(a, 1)),
            Err(GraphError::Cycle)
        );
        assert_eq!(
            g.connect(Port::new(a, 0), Port::new(a, 1)),
            Err(GraphError::Cycle)
        );
        assert_eq!(
            g.connect(Port::new(a, 2), Port::new(b, 0)),
            Err(GraphError::InvalidChannel {
                node: a,
                channel: 2
            })
        );
        assert_eq!(g.remove(g.hardware_output()), Err(GraphError::HardwareNode));
        assert!(g.disconnect(Port::new(a, 0), Port::new(b, 0)));
        assert!(g.edges().is_empty());
    }

    #[test]
    fn order_respects_dependencies() {
        let mut g = Graph::new();
        let a = g.add("a", silent(), 1, 1);
        let b = g.add("b", silent(), 1, 1);
        let c = g.add("c", silent(), 2, 1);
        g.connect(Port::new(g.hardware_input(), 0), Port::new(b, 0))
            .unwrap();
        g.connect(Port::new(b, 0), Port::new(c, 0)).unwrap();
        g.connect(Port::new(a, 0), Port::new(c, 1)).unwrap();
        g.connect(Port::new(c, 0), Port::new(g.hardware_output(), 0))
            .unwrap();
        let order = g.topological_order();
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert_eq!(order.len(), 5);
        assert!(pos(g.hardware_input()) < pos(b));
        assert!(pos(b) < pos(c) && pos(a) < pos(c));
        assert!(pos(c) < pos(g.hardware_output()));
    }

    #[test]
    fn removing_a_node_drops_its_edges() {
        let mut g = Graph::new();
        let a = g.add("a", silent(), 1, 1);
        g.connect(Port::new(a, 0), Port::new(g.hardware_output(), 0))
            .unwrap();
        g.remove(a).unwrap();
        assert!(g.edges().is_empty());
        assert!(!g.nodes.contains_key(&a));
    }
}
