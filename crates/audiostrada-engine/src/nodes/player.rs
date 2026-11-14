//! Plays audio clips placed on the timeline, following the transport.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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

/// Where a track is recording, shared by its player and its input monitor. Inside it, what
/// the track played is silent (a new take is being played over it) and its automatically
/// monitored input is heard; outside it, before a punch-in or after a punch-out, the reverse:
/// the take being redone is heard, to play along with it, as in Pro Tools.
#[derive(Debug, Default)]
pub struct RecordWindow {
    open: AtomicBool,
    /// Engine samples; `end` is `u64::MAX` until the recording stops.
    start: AtomicU64,
    end: AtomicU64,
    /// The input is heard only inside (automatic monitoring).
    gates_input: AtomicBool,
}

/// The switches at the window's edges take this long, so that they do not click.
const WINDOW_RAMP_SECONDS: f64 = 0.005;

impl RecordWindow {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn open(&self, start: u64, end: u64, gates_input: bool) {
        self.start.store(start, Ordering::Relaxed);
        self.end.store(end, Ordering::Relaxed);
        self.gates_input.store(gates_input, Ordering::Relaxed);
        self.open.store(true, Ordering::Release);
    }

    pub fn close(&self) {
        self.open.store(false, Ordering::Release);
    }

    /// The window, while open.
    pub fn range(&self) -> Option<(u64, u64)> {
        self.open.load(Ordering::Acquire).then(|| {
            (
                self.start.load(Ordering::Relaxed),
                self.end.load(Ordering::Relaxed),
            )
        })
    }

    /// The window, while open and the input heard only inside it.
    pub fn input_range(&self) -> Option<(u64, u64)> {
        self.range()
            .filter(|_| self.gates_input.load(Ordering::Relaxed))
    }
}

/// Frames of the ramps at the window's edges.
pub(crate) fn window_ramp(sample_rate: u32) -> u64 {
    ((WINDOW_RAMP_SECONDS * f64::from(sample_rate)) as u64).max(1)
}

/// How much `position` is inside `start..end`: 1 inside, 0 away from it, in between over the
/// `ramp` frames before its start and after its end.
#[inline]
pub(crate) fn inside((start, end): (u64, u64), position: u64, ramp: u64) -> f32 {
    let away = if position < start {
        start - position
    } else if position >= end {
        position - end + 1
    } else {
        return 1.0;
    };
    if away >= ramp {
        0.0
    } else {
        1.0 - away as f32 / ramp as f32
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
    /// Where the track records over what it plays.
    window: Option<Arc<RecordWindow>>,
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
                window: None,
            },
            PlayerControls {
                updates: updates_tx,
                returns: returns_rx,
            },
        )
    }
}

impl ClipPlayer {
    /// Silences the clips where the track records over them (see [`RecordWindow`]).
    pub fn with_window(mut self, window: Arc<RecordWindow>) -> Self {
        self.window = Some(window);
        self
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
        // Recorded over: the take being redone goes quiet, only the new playing is heard.
        if let Some(window) = self.window.as_ref().and_then(|w| w.range()) {
            let ramp = window_ramp(ctx.transport.sample_rate);
            if start < window.1.saturating_add(ramp) && end.saturating_add(ramp) > window.0 {
                for ch in 0..b.output_channels() {
                    for (i, d) in b.output(ch).iter_mut().enumerate() {
                        *d *= 1.0 - inside(window, start + i as u64, ramp);
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
            counting: false,
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

    #[test]
    fn a_take_recorded_over_goes_quiet_inside_the_window_only() {
        let data = Arc::new(AudioData::new(vec![vec![1.0; 48_000]], 48_000));
        let (player, _controls) = ClipPlayer::new(vec![Clip::new(0, data)]);
        let window = RecordWindow::new();
        let mut player = player.with_window(Arc::clone(&window));
        // Closed: heard everywhere.
        assert!(
            render(&mut player, 1, 1000, true)[0]
                .iter()
                .all(|&s| s == 1.0)
        );
        // A punch from 1000 to 2000: quiet inside, ramping over 5 ms (240 frames) outside.
        window.open(1000, 2000, true);
        let before = render(&mut player, 1, 700, true);
        assert_eq!(before[0][0], 1.0, "far before the punch-in");
        assert!(before[0][63] < 1.0 && before[0][63] > 0.0, "ramping down");
        assert!(
            render(&mut player, 1, 1000, true)[0]
                .iter()
                .all(|&s| s == 0.0)
        );
        assert!(
            render(&mut player, 1, 1936, true)[0]
                .iter()
                .all(|&s| s == 0.0)
        );
        let after = render(&mut player, 1, 2000, true);
        assert!(after[0][0] < 0.01, "still quiet at the punch-out");
        assert!(after[0][63] > after[0][0], "fading back in after it");
        assert_eq!(render(&mut player, 1, 2300, true)[0][0], 1.0);
        // Recording without a punch: quiet until it stops.
        window.open(1000, u64::MAX, false);
        assert!(
            render(&mut player, 1, 30_000, true)[0]
                .iter()
                .all(|&s| s == 0.0)
        );
        assert_eq!(window.input_range(), None, "the input is not gated");
        window.close();
        assert!(
            render(&mut player, 1, 30_000, true)[0]
                .iter()
                .all(|&s| s == 1.0)
        );
    }

    #[test]
    fn the_window_ramps_over_its_edges() {
        let ramp = window_ramp(48_000);
        assert_eq!(ramp, 240);
        assert_eq!(inside((1000, 2000), 1500, ramp), 1.0);
        assert_eq!(inside((1000, 2000), 1000, ramp), 1.0);
        assert_eq!(inside((1000, 2000), 1999, ramp), 1.0);
        assert_eq!(inside((1000, 2000), 500, ramp), 0.0);
        assert!((inside((1000, 2000), 880, ramp) - 0.5).abs() < 1e-6);
        assert!((inside((1000, 2000), 2119, ramp) - 0.5).abs() < 1e-6);
        assert_eq!(inside((1000, 2000), 2400, ramp), 0.0);
        assert_eq!(inside((1000, u64::MAX), u64::MAX - 1, ramp), 1.0);
    }
}
