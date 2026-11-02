//! Reference track: a finished song heard in place of the mix, at the same position in the
//! timeline and brought to the mix's loudness, to compare by ear. It sits after the master
//! meters, on the way to the outputs only: never in renders.

use std::sync::Arc;

use audiostrada_media::AudioData;
use audiostrada_rt::{Receiver, Sender, channel};

use crate::param::{SharedParam, Smoothed};
use crate::{ProcessContext, Processor};

/// UI side: the audio to compare with, whether it is heard and its gain.
#[derive(Debug)]
pub struct ReferenceControls {
    updates: Sender<Option<Arc<AudioData>>>,
    returns: Receiver<Option<Arc<AudioData>>>,
    /// 1 while the reference is heard instead of the mix.
    pub listening: SharedParam,
    pub gain_db: SharedParam,
}

impl ReferenceControls {
    /// Replaces the reference audio (at the engine's rate), `None` to drop it.
    pub fn set_audio(&mut self, audio: Option<Arc<AudioData>>) {
        self.returns.drain().for_each(drop);
        if self.updates.try_send(audio).is_err() {
            tracing::warn!("reference update queue full, change dropped");
        }
    }
}

pub struct Reference {
    audio: Option<Arc<AudioData>>,
    updates: Receiver<Option<Arc<AudioData>>>,
    returns: Sender<Option<Arc<AudioData>>>,
    listening: SharedParam,
    gain_db: SharedParam,
    /// Share of the reference in the output, ramped to switch without a click.
    blend: Smoothed,
    gain: Smoothed,
}

impl std::fmt::Debug for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reference").finish_non_exhaustive()
    }
}

impl Reference {
    pub fn new() -> (Self, ReferenceControls) {
        let (updates_tx, updates) = channel(4);
        let (returns, returns_rx) = channel(4);
        let (listening, gain_db) = (SharedParam::new(0.0), SharedParam::new(0.0));
        (
            Self {
                audio: None,
                updates,
                returns,
                listening: listening.clone(),
                gain_db: gain_db.clone(),
                blend: Smoothed::new(),
                gain: Smoothed::new(),
            },
            ReferenceControls {
                updates: updates_tx,
                returns: returns_rx,
                listening,
                gain_db,
            },
        )
    }
}

impl Processor for Reference {
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        while let Some(audio) = self.updates.try_recv() {
            let old = std::mem::replace(&mut self.audio, audio);
            let _ = self.returns.try_send(old);
        }
        let b = &mut ctx.buffers;
        for ch in 0..b.input_channels().min(b.output_channels()) {
            let (src, dst) = b.io(ch, ch);
            dst.copy_from_slice(src);
        }
        let listening = self.listening.get() >= 0.5 && self.audio.is_some();
        let frames = b.frames();
        let (blend, blend_step) = self.blend.ramp(if listening { 1.0 } else { 0.0 }, frames);
        if blend == 0.0 && blend_step == 0.0 {
            return;
        }
        let gain = 10f32.powf(self.gain_db.get() / 20.0);
        let (gain, gain_step) = self.gain.ramp(gain, frames);
        let Some(audio) = &self.audio else {
            return;
        };
        let (playing, position) = (ctx.transport.playing, ctx.transport.position);
        let length = audio.frames() as u64;
        for ch in 0..b.output_channels().min(2) {
            let source = audio.channel(ch.min(audio.channels().saturating_sub(1)));
            let out = b.output(ch);
            let (mut mix, mut level) = (blend, gain);
            for (i, s) in out.iter_mut().enumerate() {
                // Same place as in the song, looping past the end of the reference.
                let r = if playing && length > 0 {
                    source[((position + i as u64) % length) as usize] * level
                } else {
                    0.0
                };
                *s += (r - *s) * mix;
                mix += blend_step;
                level += gain_step;
            }
        }
    }
}
