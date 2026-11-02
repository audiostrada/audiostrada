//! Allocation-free logging from the audio thread.
//!
//! The audio thread pushes fixed-size events through [`RtLogger`]; a regular thread
//! periodically calls [`RtLogDrain::forward_to_tracing`] to format and emit them.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::channel::{Receiver, Sender, channel};

pub const MAX_ARGS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RtLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RtArg {
    I64(i64),
    U64(u64),
    F64(f64),
    Str(&'static str),
}

impl fmt::Display for RtArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RtArg::I64(v) => v.fmt(f),
            RtArg::U64(v) => v.fmt(f),
            RtArg::F64(v) => v.fmt(f),
            RtArg::Str(v) => v.fmt(f),
        }
    }
}

macro_rules! impl_from_arg {
    ($variant:ident, $target:ty, $($t:ty),+) => {
        $(impl From<$t> for RtArg {
            #[inline]
            fn from(v: $t) -> Self {
                RtArg::$variant(v as $target)
            }
        })+
    };
}

impl_from_arg!(I64, i64, i8, i16, i32, i64, isize);
impl_from_arg!(U64, u64, u8, u16, u32, u64, usize);
impl_from_arg!(F64, f64, f32, f64);

impl From<&'static str> for RtArg {
    #[inline]
    fn from(v: &'static str) -> Self {
        RtArg::Str(v)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RtEvent {
    pub level: RtLevel,
    pub msg: &'static str,
    args: [RtArg; MAX_ARGS],
    len: u8,
}

impl RtEvent {
    /// Builds an event, keeping at most [`MAX_ARGS`] arguments.
    #[inline]
    pub fn new(level: RtLevel, msg: &'static str, args: &[RtArg]) -> Self {
        let len = args.len().min(MAX_ARGS);
        let mut buf = [RtArg::U64(0); MAX_ARGS];
        buf[..len].copy_from_slice(&args[..len]);
        Self {
            level,
            msg,
            args: buf,
            len: len as u8,
        }
    }

    pub fn args(&self) -> &[RtArg] {
        &self.args[..self.len as usize]
    }
}

impl fmt::Display for RtEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.msg)?;
        for arg in self.args() {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// Creates a logger for one real-time thread and the drain that consumes its events.
pub fn rt_log(capacity: usize) -> (RtLogger, RtLogDrain) {
    let (tx, rx) = channel(capacity);
    let dropped = Arc::new(AtomicU64::new(0));
    (
        RtLogger {
            tx,
            dropped: Arc::clone(&dropped),
        },
        RtLogDrain {
            rx,
            dropped,
            reported_dropped: 0,
        },
    )
}

#[derive(Debug)]
pub struct RtLogger {
    tx: Sender<RtEvent>,
    dropped: Arc<AtomicU64>,
}

impl RtLogger {
    /// Queues an event without blocking; returns false if it was dropped because the queue is full.
    #[inline]
    pub fn log(&mut self, level: RtLevel, msg: &'static str, args: &[RtArg]) -> bool {
        let sent = self.tx.try_send(RtEvent::new(level, msg, args)).is_ok();
        if !sent {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        sent
    }

    #[inline]
    pub fn error(&mut self, msg: &'static str, args: &[RtArg]) -> bool {
        self.log(RtLevel::Error, msg, args)
    }

    #[inline]
    pub fn warn(&mut self, msg: &'static str, args: &[RtArg]) -> bool {
        self.log(RtLevel::Warn, msg, args)
    }

    #[inline]
    pub fn info(&mut self, msg: &'static str, args: &[RtArg]) -> bool {
        self.log(RtLevel::Info, msg, args)
    }

    #[inline]
    pub fn debug(&mut self, msg: &'static str, args: &[RtArg]) -> bool {
        self.log(RtLevel::Debug, msg, args)
    }
}

#[derive(Debug)]
pub struct RtLogDrain {
    rx: Receiver<RtEvent>,
    dropped: Arc<AtomicU64>,
    reported_dropped: u64,
}

impl RtLogDrain {
    /// Passes every pending event to `f` and returns how many were processed.
    pub fn drain_with(&mut self, mut f: impl FnMut(&RtEvent)) -> usize {
        let mut count = 0;
        for event in self.rx.drain() {
            f(&event);
            count += 1;
        }
        count
    }

    /// Emits pending events through `tracing`, reporting events lost since the last call.
    pub fn forward_to_tracing(&mut self) -> usize {
        let count = self.drain_with(|e| match e.level {
            RtLevel::Error => tracing::error!(target: "audiostrada::rt", "{e}"),
            RtLevel::Warn => tracing::warn!(target: "audiostrada::rt", "{e}"),
            RtLevel::Info => tracing::info!(target: "audiostrada::rt", "{e}"),
            RtLevel::Debug => tracing::debug!(target: "audiostrada::rt", "{e}"),
            RtLevel::Trace => tracing::trace!(target: "audiostrada::rt", "{e}"),
        });
        let dropped = self.dropped();
        if dropped > self.reported_dropped {
            tracing::warn!(
                target: "audiostrada::rt",
                lost = dropped - self.reported_dropped,
                "real-time log queue overflowed"
            );
            self.reported_dropped = dropped;
        }
        count
    }

    /// Total number of events dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_keep_level_message_and_args() {
        let (mut logger, mut drain) = rt_log(8);
        assert!(logger.warn("xrun", &[512u32.into(), 1.5f32.into()]));
        let mut seen = Vec::new();
        assert_eq!(drain.drain_with(|e| seen.push(*e)), 1);
        assert_eq!(seen[0].level, RtLevel::Warn);
        assert_eq!(seen[0].args(), &[RtArg::U64(512), RtArg::F64(1.5)]);
        assert_eq!(seen[0].to_string(), "xrun 512 1.5");
    }

    #[test]
    fn extra_args_are_truncated() {
        let args = [RtArg::I64(1); MAX_ARGS + 2];
        assert_eq!(
            RtEvent::new(RtLevel::Info, "m", &args).args().len(),
            MAX_ARGS
        );
    }

    #[test]
    fn overflow_is_counted() {
        let (mut logger, mut drain) = rt_log(1);
        assert!(logger.info("a", &[]));
        assert!(!logger.info("b", &[]));
        assert_eq!(drain.dropped(), 1);
        assert_eq!(drain.forward_to_tracing(), 1);
    }

    #[test]
    fn arguments_convert_and_display() {
        assert_eq!(RtArg::from(-3i8), RtArg::I64(-3));
        assert_eq!(RtArg::from(7usize), RtArg::U64(7));
        assert_eq!(RtArg::from(0.25f32), RtArg::F64(0.25));
        assert_eq!(RtArg::from("left"), RtArg::Str("left"));
        let event = RtEvent::new(
            RtLevel::Error,
            "lost",
            &[(-1i32).into(), 2u8.into(), "in".into()],
        );
        assert_eq!(event.to_string(), "lost -1 2 in");
        assert_eq!(
            RtEvent::new(RtLevel::Trace, "tick", &[]).to_string(),
            "tick"
        );
    }

    #[test]
    fn every_level_is_forwarded() {
        let (mut logger, mut drain) = rt_log(8);
        assert!(logger.error("e", &[]));
        assert!(logger.warn("w", &[]));
        assert!(logger.info("i", &[]));
        assert!(logger.debug("d", &[]));
        assert!(logger.log(RtLevel::Trace, "t", &[]));
        assert_eq!(drain.forward_to_tracing(), 5);
        assert_eq!(drain.forward_to_tracing(), 0);
        assert!(RtLevel::Error < RtLevel::Trace);
    }
}
