//! AudioStrada real-time safety tooling: allocation checks, lock-free channels, RT logging.

pub mod alloc;
pub mod channel;
pub mod fpu;
pub mod log;
pub mod resident;
pub mod thread;

pub use alloc::{alloc_violations, permit_alloc, rt_section};
pub use channel::{Receiver, Sender, channel};
pub use fpu::NoDenormals;
pub use log::{RtArg, RtEvent, RtLevel, RtLogDrain, RtLogger, rt_log};
pub use resident::{Resident, keep_resident, resident_bytes};
