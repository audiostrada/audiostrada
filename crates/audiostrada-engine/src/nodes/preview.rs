//! File preview: plays one decoded file from its start straight to the outputs, independently
//! of the transport, as when auditioning files in the browser.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use audiostrada_media::AudioData;
use audiostrada_rt::{Receiver, Sender, channel};

use crate::{ProcessContext, Processor};

/// UI side of the preview.
#[derive(Debug)]
pub struct PreviewControls {
    files: Sender<Option<Arc<AudioData>>>,
    garbage: Receiver<Arc<AudioData>>,
    playing: Arc<AtomicBool>,
}

impl PreviewControls {
    /// Starts playing `data` from its start, or stops with `None`.
    pub fn play(&mut self, data: Option<Arc<AudioData>>) {
        while self.garbage.try_recv().is_some() {}
        let starting = data.is_some();
        if self.files.try_send(data).is_ok() {
            self.playing.store(starting, Ordering::Relaxed);
        }
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

pub struct Preview {
    files: Receiver<Option<Arc<AudioData>>>,
    garbage: Sender<Arc<AudioData>>,
    playing: Arc<AtomicBool>,
    data: Option<Arc<AudioData>>,
    position: usize,
}

impl std::fmt::Debug for Preview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preview")
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl Preview {
    pub fn new() -> (Self, PreviewControls) {
        let (tx, files) = channel(8);
        let (garbage, garbage_rx) = channel(8);
        let playing = Arc::new(AtomicBool::new(false));
        (
            Self {
                files,
                garbage,
                playing: Arc::clone(&playing),
                data: None,
                position: 0,
            },
            PreviewControls {
                files: tx,
                garbage: garbage_rx,
                playing,
            },
        )
    }

    fn release(&mut self) {
        if let Some(old) = self.data.take()
            && let Err(old) = self.garbage.try_send(old)
        {
            audiostrada_rt::permit_alloc(|| drop(old));
        }
    }
}

impl Processor for Preview {
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        while let Some(next) = self.files.try_recv() {
            self.release();
            self.data = next;
            self.position = 0;
        }
        let b = &mut ctx.buffers;
        if b.output_channels() < 2 {
            return;
        }
        let (left, right) = b.output_pair(0, 1);
        left.fill(0.0);
        right.fill(0.0);
        let Some(data) = &self.data else {
            return;
        };
        let frames = data.frames();
        let n = left.len().min(frames.saturating_sub(self.position));
        let (a, z) = (self.position, self.position + n);
        left[..n].copy_from_slice(&data.channel(0)[a..z]);
        let second = if data.channels() > 1 { 1 } else { 0 };
        right[..n].copy_from_slice(&data.channel(second)[a..z]);
        self.position = z;
        if self.position >= frames {
            self.release();
            self.playing.store(false, Ordering::Relaxed);
        }
    }
}
