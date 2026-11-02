//! Plays audio clips placed on the timeline, following the transport.

use std::sync::Arc;

use audiostrada_core::FadeShape;
use audiostrada_media::AudioData;
use audiostrada_rt::{Receiver, Sender, channel, permit_alloc};

use crate::{ProcessContext, Processor};

const UPDATE_QUEUE: usize = 16;

#[derive(Clone, Debug)]
pub struct Clip {
    /// Timeline position of the clip start, in samples.
    pub start: u64,
    /// First frame of `data` played.
    pub offset: u64,
    pub length: u64,
    pub gain: f32,
    pub fade_in: u64,
    pub fade_out: u64,
    pub fade_shape: FadeShape,
    pub fade_out_shape: FadeShape,
    pub data: Arc<AudioData>,
}

impl Clip {
    /// The whole of `data`, starting at `start`.
    pub fn new(start: u64, data: Arc<AudioData>) -> Self {
        Self {
            start,
            offset: 0,
            length: data.frames() as u64,
            gain: 1.0,
            fade_in: 0,
            fade_out: 0,
            fade_shape: FadeShape::default(),
            fade_out_shape: FadeShape::default(),
            data,
        }
    }

    pub fn end(&self) -> u64 {
        self.start + self.length
    }

    /// Gain at `pos` samples into the clip, fades included.
    #[inline]
    fn gain_at(&self, pos: u64) -> f32 {
        let mut gain = self.gain;
        if pos < self.fade_in {
            gain *= self.fade_shape.gain(pos as f32 / self.fade_in as f32);
        }
        let to_end = self.length - pos;
        if to_end <= self.fade_out {
            gain *= self
                .fade_out_shape
                .gain(to_end as f32 / self.fade_out as f32);
        }
        gain
    }
}

/// UI side of a player: replaces its clip list without blocking the audio thread.
#[derive(Debug)]
pub struct PlayerControls {
    updates: Sender<Arc<[Clip]>>,
    returns: Receiver<Arc<[Clip]>>,
}

impl PlayerControls {
    pub fn set_clips(&mut self, clips: Vec<Clip>) {
        self.collect();
        if self.updates.try_send(Arc::from(clips)).is_err() {
            tracing::warn!("player update queue full, clip change dropped");
        }
    }

    /// Frees clip lists the audio thread is done with.
    pub fn collect(&mut self) {
        self.returns.drain().for_each(drop);
    }
}

#[derive(Debug)]
pub struct ClipPlayer {
    clips: Arc<[Clip]>,
    updates: Receiver<Arc<[Clip]>>,
    returns: Sender<Arc<[Clip]>>,
}

impl ClipPlayer {
    pub fn new(clips: Vec<Clip>) -> (Self, PlayerControls) {
        let (updates_tx, updates) = channel(UPDATE_QUEUE);
        let (returns, returns_rx) = channel(UPDATE_QUEUE);
        (
            Self {
                clips: Arc::from(clips),
                updates,
                returns,
            },
            PlayerControls {
                updates: updates_tx,
                returns: returns_rx,
            },
        )
    }
}

impl Processor for ClipPlayer {
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        while let Some(clips) = self.updates.try_recv() {
            let old = std::mem::replace(&mut self.clips, clips);
            if let Err(old) = self.returns.try_send(old) {
                permit_alloc(|| drop(old));
            }
        }
        let b = &mut ctx.buffers;
        b.clear_outputs();
        if !ctx.transport.playing {
            return;
        }
        let start = ctx.transport.position;
        let end = start + b.frames() as u64;
        for clip in self
            .clips
            .iter()
            .filter(|c| c.start < end && c.end() > start)
        {
            let from = start.max(clip.start);
            let to = end.min(clip.end());
            let n = (to - from) as usize;
            let out_at = (from - start) as usize;
            let data_at = (clip.offset + from - clip.start) as usize;
            let data = &clip.data;
            if data_at >= data.frames() {
                continue;
            }
            let n = n.min(data.frames() - data_at);
            let rel = from - clip.start;
            let in_fade = rel < clip.fade_in || clip.length - (rel + n as u64) < clip.fade_out;
            for ch in 0..b.output_channels() {
                let source = match data.channels() {
                    0 => break,
                    1 => 0,
                    c if ch < c => ch,
                    _ => continue,
                };
                let src = &data.channel(source)[data_at..data_at + n];
                let dst = &mut b.output(ch)[out_at..out_at + n];
                if in_fade {
                    for (i, (d, s)) in dst.iter_mut().zip(src).enumerate() {
                        *d += s * clip.gain_at(rel + i as u64);
                    }
                } else {
                    for (d, s) in dst.iter_mut().zip(src) {
                        *d += s * clip.gain;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_shape_the_clip_edges() {
        let data = Arc::new(AudioData::new(vec![vec![1.0; 100]], 48_000));
        let clip = Clip {
            fade_in: 10,
            fade_out: 20,
            fade_shape: FadeShape::Linear,
            fade_out_shape: FadeShape::Linear,
            ..Clip::new(0, data)
        };
        assert_eq!(clip.gain_at(0), 0.0);
        assert!((clip.gain_at(5) - 0.5).abs() < 1e-6);
        assert_eq!(clip.gain_at(50), 1.0);
        assert!((clip.gain_at(90) - 0.5).abs() < 1e-6);
        assert!((clip.gain_at(99) - 0.05).abs() < 1e-6);
    }

    /// Outputs of one 64-frame block at `position`.
    fn render(
        player: &mut ClipPlayer,
        outputs: usize,
        position: u64,
        playing: bool,
    ) -> Vec<Vec<f32>> {
        let transport = crate::TransportInfo {
            playing,
            position,
            sample_rate: 48_000,
            beat: 0.0,
            bpm: 120.0,
            bar: Default::default(),
        };
        let mut storage = audiostrada_io::ChannelBuffers::new(0, outputs, 64);
        player.process(&mut ProcessContext {
            buffers: storage.buffers(64),
            transport: &transport,
        });
        (0..outputs).map(|c| storage.output(c).to_vec()).collect()
    }

    #[test]
    fn clips_play_their_channels_within_their_data() {
        let mono = Arc::new(AudioData::new(vec![vec![0.5; 30]], 48_000));
        let longer = Clip {
            length: 50,
            gain: 2.0,
            ..Clip::new(10, mono)
        };
        let (mut player, mut controls) = ClipPlayer::new(vec![longer]);
        let out = render(&mut player, 2, 0, true);
        let expected: Vec<f32> = (0..64)
            .map(|i| if (10..40).contains(&i) { 1.0 } else { 0.0 })
            .collect();
        assert_eq!(out, [expected.clone(), expected], "mono feeds both sides");
        assert!(
            render(&mut player, 2, 0, false)[0]
                .iter()
                .all(|s| *s == 0.0)
        );
        let stereo = Arc::new(AudioData::new(
            vec![vec![0.25; 200], vec![-0.25; 200]],
            48_000,
        ));
        controls.set_clips(vec![Clip {
            offset: 100,
            length: 100,
            ..Clip::new(0, stereo)
        }]);
        let out = render(&mut player, 3, 64, true);
        assert!(out[0][..36].iter().all(|s| *s == 0.25) && out[0][36..].iter().all(|s| *s == 0.0));
        assert!(out[1][..36].iter().all(|s| *s == -0.25));
        assert!(
            out[2].iter().all(|s| *s == 0.0),
            "no third channel in the file"
        );
        controls.collect();
    }

    #[test]
    fn equal_power_crossfade_keeps_power() {
        let shape = FadeShape::EqualPower;
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let (fade_in, fade_out) = (shape.gain(t), shape.gain(1.0 - t));
            assert!((fade_in * fade_in + fade_out * fade_out - 1.0).abs() < 1e-5);
        }
    }
}
