//! Broadcast ring for live MIDI input: one writer (the device thread), any number of readers
//! (audio nodes, the recorder), each with its own read position. Wait-free; a reader that falls
//! more than the ring behind skips ahead.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const SIZE: usize = 1024;

/// Packs a message and its arrival position (engine samples, 40 bits).
fn pack(data: [u8; 3], position: u64) -> u64 {
    u64::from(data[0])
        | u64::from(data[1]) << 8
        | u64::from(data[2]) << 16
        | (position & 0xFF_FFFF_FFFF) << 24
}

fn unpack(value: u64) -> ([u8; 3], u64) {
    (
        [value as u8, (value >> 8) as u8, (value >> 16) as u8],
        value >> 24,
    )
}

#[derive(Debug)]
struct Ring {
    slots: Box<[AtomicU64]>,
    written: AtomicU64,
}

/// Writer side.
#[derive(Debug, Clone)]
pub struct LiveMidi {
    ring: Arc<Ring>,
}

/// A reader with its own position.
#[derive(Debug)]
pub struct LiveReader {
    ring: Arc<Ring>,
    read: u64,
}

impl Default for LiveMidi {
    fn default() -> Self {
        Self {
            ring: Arc::new(Ring {
                slots: (0..SIZE).map(|_| AtomicU64::new(0)).collect(),
                written: AtomicU64::new(0),
            }),
        }
    }
}

impl LiveMidi {
    /// Publishes a message that arrived at `position` (engine samples).
    pub fn push(&self, data: [u8; 3], position: u64) {
        let n = self.ring.written.load(Ordering::Relaxed);
        self.ring.slots[(n % SIZE as u64) as usize].store(pack(data, position), Ordering::Relaxed);
        self.ring.written.store(n + 1, Ordering::Release);
    }

    /// Whether both write to the same ring.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ring, &other.ring)
    }

    /// A reader starting at the newest message.
    pub fn reader(&self) -> LiveReader {
        LiveReader {
            ring: Arc::clone(&self.ring),
            read: self.ring.written.load(Ordering::Acquire),
        }
    }
}

impl LiveReader {
    /// Calls `each` for every message published since the last call.
    pub fn drain(&mut self, mut each: impl FnMut([u8; 3], u64)) {
        let written = self.ring.written.load(Ordering::Acquire);
        if written - self.read > SIZE as u64 {
            self.read = written - SIZE as u64;
        }
        while self.read < written {
            let (data, position) =
                unpack(self.ring.slots[(self.read % SIZE as u64) as usize].load(Ordering::Relaxed));
            each(data, position);
            self.read += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reader_sees_every_message() {
        let live = LiveMidi::default();
        let mut a = live.reader();
        live.push([0x90, 60, 100], 1234);
        let mut b = live.reader();
        live.push([0x80, 60, 0], 99_999_999);
        let mut seen = Vec::new();
        a.drain(|d, p| seen.push((d, p)));
        assert_eq!(
            seen,
            vec![([0x90, 60, 100], 1234), ([0x80, 60, 0], 99_999_999)]
        );
        let mut late = Vec::new();
        b.drain(|d, _| late.push(d));
        assert_eq!(late, vec![[0x80, 60, 0]]);
        for i in 0..3000u64 {
            live.push([0xB0, 1, (i % 128) as u8], i);
        }
        let mut count = 0;
        a.drain(|_, _| count += 1);
        assert_eq!(count, 1024, "a slow reader skips ahead");
    }
}
