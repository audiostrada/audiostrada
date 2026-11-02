use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Transport and engine state published by the audio thread for the UI.
#[derive(Debug, Default)]
pub struct EngineShared {
    playing: AtomicBool,
    position: AtomicU64,
    latency: AtomicU32,
}

impl EngineShared {
    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Timeline position in samples at the end of the last processed block.
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    /// Latency added by the graph (plugin delay compensation), in samples.
    pub fn latency(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }

    pub(crate) fn publish(&self, playing: bool, position: u64) {
        self.playing.store(playing, Ordering::Relaxed);
        self.position.store(position, Ordering::Relaxed);
    }

    pub(crate) fn set_latency(&self, latency: u32) {
        self.latency.store(latency, Ordering::Relaxed);
    }
}

/// A run of frames with a continuous timeline position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Segment {
    /// Offset of the first frame in the device block.
    pub offset: usize,
    pub frames: usize,
    pub position: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Transport {
    pub playing: bool,
    pub position: u64,
    /// Loop range `[start, end)`, active while playing before `end`.
    pub looping: Option<(u64, u64)>,
}

impl Transport {
    /// Splits a block of `frames` into segments no longer than `max_frames`, wrapping at the
    /// loop end, and advances the position.
    pub fn advance(&mut self, frames: usize, max_frames: usize, mut each: impl FnMut(Segment)) {
        let mut offset = 0;
        while offset < frames {
            let mut len = (frames - offset).min(max_frames.max(1));
            let loop_range = self
                .looping
                .filter(|&(start, end)| self.playing && start < end && self.position < end);
            if let Some((_, end)) = loop_range {
                len = len.min((end - self.position) as usize);
            }
            each(Segment {
                offset,
                frames: len,
                position: self.position,
            });
            offset += len;
            if self.playing {
                self.position += len as u64;
                if let Some((start, end)) = loop_range
                    && self.position >= end
                {
                    self.position = start;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(t: &mut Transport, frames: usize, max: usize) -> Vec<Segment> {
        let mut out = Vec::new();
        t.advance(frames, max, |s| out.push(s));
        out
    }

    #[test]
    fn stopped_transport_does_not_move() {
        let mut t = Transport {
            playing: false,
            position: 100,
            looping: Some((0, 50)),
        };
        assert_eq!(
            segments(&mut t, 64, 1024),
            vec![Segment {
                offset: 0,
                frames: 64,
                position: 100
            }]
        );
        assert_eq!(t.position, 100);
    }

    #[test]
    fn loop_wraps_inside_a_block() {
        let mut t = Transport {
            playing: true,
            position: 90,
            looping: Some((40, 100)),
        };
        assert_eq!(
            segments(&mut t, 32, 1024),
            vec![
                Segment {
                    offset: 0,
                    frames: 10,
                    position: 90
                },
                Segment {
                    offset: 10,
                    frames: 22,
                    position: 40
                },
            ]
        );
        assert_eq!(t.position, 62);
    }

    #[test]
    fn short_loops_wrap_several_times() {
        let mut t = Transport {
            playing: true,
            position: 0,
            looping: Some((0, 10)),
        };
        let s = segments(&mut t, 25, 1024);
        assert_eq!(
            s.iter().map(|s| s.frames).collect::<Vec<_>>(),
            vec![10, 10, 5]
        );
        assert_eq!(t.position, 5);
    }

    #[test]
    fn position_past_loop_plays_through() {
        let mut t = Transport {
            playing: true,
            position: 200,
            looping: Some((0, 100)),
        };
        assert_eq!(segments(&mut t, 16, 1024).len(), 1);
        assert_eq!(t.position, 216);
    }

    #[test]
    fn oversized_blocks_are_split() {
        let mut t = Transport {
            playing: true,
            position: 0,
            looping: None,
        };
        let s = segments(&mut t, 2500, 1024);
        assert_eq!(
            s.iter().map(|s| s.frames).collect::<Vec<_>>(),
            vec![1024, 1024, 452]
        );
        assert_eq!(s[2].offset, 2048);
        assert_eq!(t.position, 2500);
    }
}
