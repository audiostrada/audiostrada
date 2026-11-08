//! Insert slot: runs a built-in effect in place, parameters shared with the UI.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use audiostrada_dsp::Effect;

use crate::nodes::levels::{LevelControls, Levels};
use crate::nodes::{MidiLayer, MidiPlayer};
use crate::param::SharedParam;
use crate::{ProcessContext, Processor};

/// UI side of an insert.
#[derive(Clone, Debug)]
pub struct InsertControls {
    pub params: Arc<[SharedParam]>,
    bypass: Arc<AtomicBool>,
    /// Values reported by the effect for display.
    pub meters: Arc<[SharedParam]>,
    /// The key inputs (channels 2 and 3) carry a sidechain signal.
    pub keyed: Arc<AtomicBool>,
    pub levels: LevelControls,
}

impl InsertControls {
    pub fn set_bypass(&self, bypass: bool) {
        self.bypass.store(bypass, Ordering::Relaxed);
    }
}

pub struct Insert {
    effect: Box<dyn Effect>,
    controls: InsertControls,
    applied: Vec<f32>,
    meters: Vec<f32>,
    midi: Option<MidiPlayer>,
    /// An instrument layered on the track's lead: its notes, and the sound it is given,
    /// kept aside to be added back under its own.
    layer: Option<MidiLayer>,
    under: [Vec<f32>; 2],
    /// Copy of the key inputs for the block.
    key: [Vec<f32>; 2],
    levels: Levels,
}

impl std::fmt::Debug for Insert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Insert")
            .field("controls", &self.controls)
            .finish_non_exhaustive()
    }
}

impl Insert {
    /// `values` are the initial parameter values, in the effect's parameter order; `meters` is
    /// the number of values the effect reports.
    pub fn new(effect: Box<dyn Effect>, values: &[f32], meters: usize) -> (Self, InsertControls) {
        // Meters start at what the effect reports at rest (silence), not at 0 dB.
        let mut rest = vec![0.0; meters];
        effect.meters(&mut rest);
        let (levels, level_controls) = Levels::new();
        let controls = InsertControls {
            params: values.iter().map(|&v| SharedParam::new(v)).collect(),
            bypass: Arc::new(AtomicBool::new(false)),
            meters: rest.iter().map(|&v| SharedParam::new(v)).collect(),
            keyed: Arc::new(AtomicBool::new(false)),
            levels: level_controls,
        };
        (
            Self {
                effect,
                controls: controls.clone(),
                applied: vec![f32::NAN; values.len()],
                meters: vec![0.0; meters],
                midi: None,
                layer: None,
                under: [Vec::new(), Vec::new()],
                key: [Vec::new(), Vec::new()],
                levels,
            },
            controls,
        )
    }
}

impl Insert {
    /// Gives the insert the track's MIDI (instrument slot).
    pub fn with_midi(mut self, midi: Option<MidiPlayer>) -> Self {
        self.midi = midi;
        self
    }

    /// Layers the insert on the track's lead instrument: it plays the lead's notes, over the
    /// sound it is given.
    pub fn with_layer(mut self, layer: Option<MidiLayer>) -> Self {
        self.layer = layer;
        self
    }
}

/// Processes a segment, listening to the sidechain key when there is one.
fn run(
    effect: &mut dyn Effect,
    left: &mut [f32],
    right: &mut [f32],
    key: Option<(&[f32], &[f32])>,
) {
    match key {
        Some(key) => effect.process_keyed(left, right, key),
        None => effect.process(left, right),
    }
}

impl Processor for Insert {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.effect.prepare(sample_rate, max_frames);
        self.levels.prepare(sample_rate);
        self.key = [vec![0.0; max_frames], vec![0.0; max_frames]];
        self.under = [vec![0.0; max_frames], vec![0.0; max_frames]];
        self.applied.fill(f32::NAN);
    }

    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        for ch in 0..b.input_channels().min(b.output_channels()) {
            let (src, dst) = b.io(ch, ch);
            dst.copy_from_slice(src);
        }
        if b.output_channels() < 2 {
            return;
        }
        if self.controls.bypass.load(Ordering::Relaxed) {
            // A lead bypassed still hands its notes to the instruments layered on it.
            if let Some(midi) = &mut self.midi {
                midi.collect(ctx.transport, b.frames());
            }
            if let Some(layer) = &mut self.layer {
                layer.collect();
            }
            let (left, right) = b.output_pair(0, 1);
            self.levels.bypassed(left, right);
            return;
        }
        for (i, (param, applied)) in self
            .controls
            .params
            .iter()
            .zip(&mut self.applied)
            .enumerate()
        {
            let value = param.get();
            if value.to_bits() != applied.to_bits() {
                *applied = value;
                self.effect.set_param(i, value);
            }
        }
        self.effect.set_tempo(ctx.transport.bpm);
        let frames = b.frames().min(self.key[0].len());
        let keyed = self.controls.keyed.load(Ordering::Relaxed) && b.input_channels() >= 4;
        if keyed {
            for (k, ch) in self.key.iter_mut().zip([2, 3]) {
                k[..frames].copy_from_slice(&b.input(ch)[..frames]);
            }
        }
        let (left, right) = b.output_pair(0, 1);
        self.levels.input(left, right);
        // Whatever path the block takes (notes, sidechain, plain): an effect that measures for
        // its window (the channel strip's spectra) does so while the window is open.
        self.effect.set_watched(self.levels.watched());
        // Layered: the sound so far waits aside, the instrument starts from silence.
        let layered = self.layer.is_some() && self.under[0].len() >= left.len();
        if layered {
            self.under[0][..left.len()].copy_from_slice(left);
            self.under[1][..right.len()].copy_from_slice(right);
            left.fill(0.0);
            right.fill(0.0);
        }
        let notes = match (&mut self.midi, &mut self.layer) {
            (Some(midi), _) => {
                let n = if keyed {
                    left.len().min(frames)
                } else {
                    left.len()
                };
                Some((midi.collect(ctx.transport, n), n))
            }
            (None, Some(layer)) => Some((layer.collect(), left.len())),
            (None, None) => None,
        };
        match notes {
            Some((events, n)) => {
                let mut cursor = 0;
                for event in events {
                    let t = (event.time as usize).min(n);
                    if t > cursor {
                        run(
                            self.effect.as_mut(),
                            &mut left[cursor..t],
                            &mut right[cursor..t],
                            keyed.then(|| (&self.key[0][cursor..t], &self.key[1][cursor..t])),
                        );
                        cursor = t;
                    }
                    self.effect.midi(event);
                }
                if cursor < n {
                    run(
                        self.effect.as_mut(),
                        &mut left[cursor..n],
                        &mut right[cursor..n],
                        keyed.then(|| (&self.key[0][cursor..n], &self.key[1][cursor..n])),
                    );
                }
            }
            None if keyed => {
                let n = left.len().min(frames);
                self.effect.process_keyed(
                    &mut left[..n],
                    &mut right[..n],
                    (&self.key[0][..n], &self.key[1][..n]),
                );
            }
            None => self.effect.process(left, right),
        }
        if layered {
            for (s, u) in left.iter_mut().zip(&self.under[0]) {
                *s += u;
            }
            for (s, u) in right.iter_mut().zip(&self.under[1]) {
                *s += u;
            }
        }
        // A broken value would stick in every filter after it: silence and restart the effect.
        if left.iter().chain(right.iter()).any(|s| !s.is_finite()) {
            left.fill(0.0);
            right.fill(0.0);
            self.effect.reset();
        }
        self.levels.output(left, right);
        if !self.meters.is_empty() {
            self.effect.meters(&mut self.meters);
            for (shared, &value) in self.controls.meters.iter().zip(&self.meters) {
                shared.set(value);
            }
        }
    }

    fn reset(&mut self) {
        self.effect.reset();
        self.levels.reset();
    }
}
