//! AudioStrada real-time safety tooling: allocation checks, lock-free channels, RT logging.

pub mod alloc;
pub mod channel;
pub mod log;
pub mod thread;

pub use alloc::{alloc_violations, permit_alloc, rt_section};
pub use channel::{Receiver, Sender, channel};
pub use log::{RtArg, RtEvent, RtLevel, RtLogDrain, RtLogger, rt_log};
