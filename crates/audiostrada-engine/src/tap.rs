//! Read access to hardware inputs from another thread, recording or not: the audio thread
//! copies the chosen channels into lock-free rings.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use audiostrada_io::AudioBuffers;
use audiostrada_rt::{Receiver, Sender, channel};

const BLOCK_CAPACITY: usize = 4096;

/// A block of input frames: where the transport was and whether it was playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TapBlock {
    pub position: u64,
    pub frames: u32,
    pub playing: bool,
}

/// Audio-thread side of a tap.
pub(crate) struct TapWriter {
    channels: Box<[usize]>,
    samples: Sender<f32>,
    blocks: Sender<TapBlock>,
    dropped: Arc<AtomicU64>,
}

impl TapWriter {
    pub(crate) fn write(
        &mut self,
        buffers: &AudioBuffers<'_>,
        offset: usize,
        frames: usize,
        position: u64,
        playing: bool,
    ) {
        if self.samples.free_slots() < frames * self.channels.len() || self.blocks.free_slots() == 0
        {
            self.dropped.fetch_add(frames as u64, Ordering::Relaxed);
            return;
        }
        let inputs = buffers.input_channels();
        for i in offset..offset + frames {
            for &ch in self.channels.iter() {
                let sample = if ch < inputs {
                    buffers.input(ch)[i]
                } else {
                    0.0
                };
                let _ = self.samples.try_send(sample);
            }
        }
        let _ = self.blocks.try_send(TapBlock {
            position,
            frames: frames as u32,
            playing,
        });
    }
}

/// Reading side of a tap, kept by whoever listens.
pub struct InputTap {
    channels: Vec<usize>,
    samples: Receiver<f32>,
    blocks: Receiver<TapBlock>,
    dropped: Arc<AtomicU64>,
    frame: Vec<f32>,
}

impl std::fmt::Debug for InputTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputTap")
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl InputTap {
    /// Hardware input channels, in the order of the interleaved samples.
    pub fn channels(&self) -> &[usize] {
        &self.channels
    }

    /// Frames lost because the reader did not keep up.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Calls `each` with every block written since the last call and each of its frames,
    /// one sample per channel.
    pub fn read(&mut self, mut each: impl FnMut(TapBlock, &[f32])) {
        while let Some(block) = self.blocks.try_recv() {
            for _ in 0..block.frames {
                for s in self.frame.iter_mut() {
                    *s = self.samples.try_recv().unwrap_or(0.0);
                }
                each(block, &self.frame);
            }
        }
    }
}

pub(crate) fn open(
    channels: Vec<usize>,
    sample_rate: u32,
    seconds: f32,
) -> (Box<TapWriter>, InputTap) {
    let frames = (sample_rate as f32 * seconds.max(0.1)) as usize;
    let (samples, samples_rx) = channel(frames * channels.len().max(1));
    let (blocks, blocks_rx) = channel(BLOCK_CAPACITY);
    let dropped = Arc::new(AtomicU64::new(0));
    let writer = Box::new(TapWriter {
        channels: channels.clone().into_boxed_slice(),
        samples,
        blocks,
        dropped: Arc::clone(&dropped),
    });
    let frame = vec![0.0; channels.len()];
    (
        writer,
        InputTap {
            channels,
            samples: samples_rx,
            blocks: blocks_rx,
            dropped,
            frame,
        },
    )
}

#[cfg(test)]
mod tests {
    use audiostrada_io::ChannelBuffers;

    use crate::{Engine, EngineConfig};

    #[test]
    fn a_tap_hears_the_inputs_stopped_and_playing() {
        let mut engine = Engine::new(EngineConfig { workers: Some(0) });
        let mut processor = engine.processor(48_000, 64);
        let mut tap = engine.open_input_tap(vec![1, 3], 1.0).unwrap();
        let mut storage = ChannelBuffers::new(2, 2, 64);
        let mut n = 0.0;
        for block in 0..4 {
            if block == 2 {
                engine.play();
            }
            for i in 0..64 {
                storage.input_mut(1)[i] = n;
                n += 1.0;
            }
            processor.render(&mut storage.buffers(64));
        }
        let mut seen = Vec::new();
        let mut blocks = Vec::new();
        tap.read(|block, frame| {
            seen.push((frame[0], frame[1]));
            if blocks.last() != Some(&block) {
                blocks.push(block);
            }
        });
        assert_eq!(seen.len(), 256);
        assert!(seen.iter().enumerate().all(|(i, &f)| f == (i as f32, 0.0)));
        assert_eq!(blocks.iter().filter(|b| b.playing).count(), 2);
        assert_eq!(blocks.last().map(|b| b.position), Some(64));
        assert_eq!(tap.dropped(), 0);
        engine.close_input_tap();
        processor.render(&mut storage.buffers(64));
        engine.collect_garbage();
        tap.read(|_, _| panic!("closed"));
    }
}
