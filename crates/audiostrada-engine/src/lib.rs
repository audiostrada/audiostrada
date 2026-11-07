//! AudioStrada real-time audio graph, scheduler and transport.

pub mod automation;
mod engine;
pub mod evaluation;
pub mod export;
mod graph;
pub mod loudness;
pub mod media;
pub mod meter;
pub mod nodes;
mod param;
mod pool;
mod processor;
pub mod project;
pub mod record;
mod schedule;
pub mod tap;
mod transport;

pub use audiostrada_core::tempo;

pub use engine::{Engine, EngineConfig, EngineProcessor};
pub use graph::{Edge, Graph, GraphError, HARDWARE_CHANNELS, NodeId, NodeKind, Port};
pub use param::{SharedParam, Smoothed};
pub use processor::{BarPosition, ProcessContext, Processor, TransportInfo};
pub use transport::EngineShared;
