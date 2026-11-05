use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

use audiostrada_rt::{alloc_violations, rt_section};

use crate::{AudioBuffers, StreamInfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackTime {
    /// Frames processed since the stream started.
    pub position: u64,
    pub sample_rate: u32,
}

/// Real-time audio processing, called once per cycle on the audio thread.
pub trait AudioCallback: Send + 'static {
    fn process(&mut self, buffers: &mut AudioBuffers<'_>, time: &CallbackTime);
}

impl<F> AudioCallback for F
where
    F: FnMut(&mut AudioBuffers<'_>, &CallbackTime) + Send + 'static,
{
    fn process(&mut self, buffers: &mut AudioBuffers<'_>, time: &CallbackTime) {
        self(buffers, time)
    }
}

/// A running audio stream; dropping it stops the stream.
pub trait AudioStream: Send + fmt::Debug {
    fn info(&self) -> &StreamInfo;
    fn status(&self) -> &StreamStatus;
}

/// A stream served by a thread of its own, which runs while `running` is set; stopped on drop.
#[derive(Debug)]
pub(crate) struct ThreadStream {
    pub info: StreamInfo,
    pub status: Arc<StreamStatus>,
    pub running: Arc<AtomicBool>,
    pub thread: Option<JoinHandle<()>>,
}

impl AudioStream for ThreadStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }

    fn status(&self) -> &StreamStatus {
        &self.status
    }
}

impl Drop for ThreadStream {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Counters updated by the audio thread, readable from any thread.
#[derive(Debug, Default)]
pub struct StreamStatus {
    cycles: AtomicU64,
    xruns: AtomicU64,
    alloc_violations: AtomicU64,
    panicked: AtomicBool,
    failed: AtomicBool,
    frames: AtomicU32,
    sample_rate: AtomicU32,
    /// Processing time over the buffer's duration: averaged, and the highest since read.
    load: AtomicU32,
    load_peak: AtomicU32,
}

impl StreamStatus {
    pub fn cycles(&self) -> u64 {
        self.cycles.load(Ordering::Relaxed)
    }

    pub fn xruns(&self) -> u64 {
        self.xruns.load(Ordering::Relaxed)
    }

    /// Allocations detected in the callback (debug builds only).
    pub fn alloc_violations(&self) -> u64 {
        self.alloc_violations.load(Ordering::Relaxed)
    }

    /// True once the callback has panicked; the stream then outputs silence.
    pub fn panicked(&self) -> bool {
        self.panicked.load(Ordering::Relaxed)
    }

    /// True once the backend stopped the stream after an unrecoverable error (device lost...).
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) fn set_failed(&self) {
        self.failed.store(true, Ordering::Relaxed);
    }

    /// Frames in the last cycle, which may differ from the requested buffer size.
    pub fn current_buffer_size(&self) -> u32 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn current_sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Share of each buffer's time spent processing, averaged over about half a second.
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }

    /// Highest share of a buffer's time spent processing since the last call.
    pub fn take_load_peak(&self) -> f32 {
        f32::from_bits(self.load_peak.swap(0, Ordering::Relaxed))
    }

    fn add_load(&self, average: f32, load: f32) {
        self.load.store(average.to_bits(), Ordering::Relaxed);
        self.load_peak.fetch_max(load.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn add_xruns(&self, count: u64) {
        self.xruns.fetch_add(count, Ordering::Relaxed);
    }
}

/// Runs the user callback with RT checks, panic isolation and status accounting.
pub(crate) struct CallbackRunner {
    callback: Box<dyn AudioCallback>,
    status: Arc<StreamStatus>,
    sample_rate: u32,
    position: u64,
    load: f32,
}

impl CallbackRunner {
    pub(crate) fn new(
        callback: Box<dyn AudioCallback>,
        status: Arc<StreamStatus>,
        sample_rate: u32,
    ) -> Self {
        Self {
            callback,
            status,
            sample_rate,
            position: 0,
            load: 0.0,
        }
    }

    /// Replaces the callback, for backends that only know the stream format on their own thread.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn set_callback(&mut self, callback: Box<dyn AudioCallback>) {
        self.callback = callback;
    }

    pub(crate) fn status(&self) -> &StreamStatus {
        &self.status
    }

    #[cfg_attr(
        not(all(target_os = "linux", any(feature = "pipewire", feature = "jack"))),
        allow(dead_code)
    )]
    pub(crate) fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate;
    }

    pub(crate) fn run(&mut self, buffers: &mut AudioBuffers<'_>) {
        let start = Instant::now();
        if self.status.panicked() {
            buffers.clear_outputs();
        } else {
            let time = CallbackTime {
                position: self.position,
                sample_rate: self.sample_rate,
            };
            let before = alloc_violations();
            let callback = &mut self.callback;
            let result = catch_unwind(AssertUnwindSafe(|| {
                rt_section(|| callback.process(buffers, &time))
            }));
            let violations = alloc_violations().wrapping_sub(before);
            if violations > 0 {
                self.status
                    .alloc_violations
                    .fetch_add(u64::from(violations), Ordering::Relaxed);
            }
            if result.is_err() {
                self.status.panicked.store(true, Ordering::Relaxed);
                buffers.clear_outputs();
            }
        }
        let frames = buffers.frames();
        let span = frames as f32 / self.sample_rate.max(1) as f32;
        if span > 0.0 {
            let load = start.elapsed().as_secs_f32() / span;
            let k = (span / 0.5).min(1.0);
            self.load += k * (load - self.load);
            self.status.add_load(self.load, load);
        }
        self.position += frames as u64;
        self.status.frames.store(frames as u32, Ordering::Relaxed);
        self.status
            .sample_rate
            .store(self.sample_rate, Ordering::Relaxed);
        self.status.cycles.fetch_add(1, Ordering::Relaxed);
    }
}

impl fmt::Debug for CallbackRunner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackRunner")
            .field("sample_rate", &self.sample_rate)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{CallbackTime, ChannelBuffers};

    #[test]
    fn runner_counts_frames_and_isolates_panics() {
        let status = Arc::new(StreamStatus::default());
        let mut called = false;
        let callback = move |b: &mut AudioBuffers<'_>, t: &CallbackTime| {
            assert!(!std::mem::replace(&mut called, true), "second call");
            b.output(0).fill(t.position as f32 + 1.0);
        };
        let mut runner = CallbackRunner::new(Box::new(callback), Arc::clone(&status), 44_100);
        let mut storage = ChannelBuffers::new(0, 1, 16);
        runner.run(&mut storage.buffers(16));
        assert_eq!(storage.output(0)[0], 1.0);
        runner.set_sample_rate(48_000);
        runner.run(&mut storage.buffers(8));
        assert!(status.panicked());
        assert_eq!(&storage.output(0)[..8], &[0.0; 8]);
        assert_eq!(storage.output(0)[8], 1.0);
        runner.run(&mut storage.buffers(4));
        assert_eq!(status.cycles(), 3);
        assert_eq!(status.current_buffer_size(), 4);
        assert_eq!(status.current_sample_rate(), 48_000);
        status.add_xruns(2);
        status.set_failed();
        assert_eq!(status.xruns(), 2);
        assert!(status.failed());
        assert!(format!("{runner:?}").contains("position: 28"));
    }

    #[test]
    fn the_load_is_the_share_of_the_buffer_spent_processing() {
        let status = Arc::new(StreamStatus::default());
        // 1000 frames at 100 kHz last 10 ms; the callback works for about 6 of them.
        let busy = |_: &mut AudioBuffers<'_>, _: &CallbackTime| {
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(6) {}
        };
        let mut runner = CallbackRunner::new(Box::new(busy), Arc::clone(&status), 100_000);
        let mut storage = ChannelBuffers::new(0, 1, 1000);
        for _ in 0..200 {
            runner.run(&mut storage.buffers(1000));
        }
        assert!((0.55..1.5).contains(&status.load()), "{}", status.load());
        assert!(status.take_load_peak() >= 0.6);
        assert_eq!(status.take_load_peak(), 0.0);
    }

    #[test]
    fn replaced_callback_runs_next_cycle() {
        let status = Arc::new(StreamStatus::default());
        let silence = |b: &mut AudioBuffers<'_>, _: &CallbackTime| b.clear_outputs();
        let mut runner = CallbackRunner::new(Box::new(silence), status, 48_000);
        let mut storage = ChannelBuffers::new(0, 1, 4);
        runner.set_callback(Box::new(|b: &mut AudioBuffers<'_>, _: &CallbackTime| {
            b.output(0).fill(0.5)
        }));
        runner.run(&mut storage.buffers(4));
        assert_eq!(storage.output(0), &[0.5; 4]);
    }
}
