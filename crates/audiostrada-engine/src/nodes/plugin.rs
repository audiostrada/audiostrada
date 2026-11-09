//! Insert slot running an external plugin. Parameter changes travel through wait-free queues in
//! both directions.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use audiostrada_plugins::PluginProcessor;
use audiostrada_rt::{Receiver, Sender, channel};

use crate::nodes::levels::{LevelControls, Levels};
use crate::nodes::{MidiLayer, MidiPlayer};
use crate::param::SharedParam;
use crate::{ProcessContext, Processor};

const QUEUE: usize = 1024;
const MAX_CHANGES: usize = 256;
/// Plugin parameters that automation can drive at once, per insert.
pub const AUTOMATION_SLOTS: usize = 64;

/// A plugin parameter driven by automation.
#[derive(Debug)]
pub struct AutomationSlot {
    pub id: AtomicU32,
    pub value: SharedParam,
    pub active: AtomicBool,
}

/// UI side of a plugin slot.
#[derive(Debug)]
pub struct PluginControls {
    pub to_plugin: Sender<(u32, f64)>,
    pub from_plugin: Receiver<(u32, f64)>,
    bypass: Arc<AtomicBool>,
    pub automation: Arc<[AutomationSlot]>,
    pub levels: LevelControls,
    /// The key inputs (channels 2 and 3) carry a sidechain signal.
    pub keyed: Arc<AtomicBool>,
}

impl PluginControls {
    pub fn set_bypass(&self, bypass: bool) {
        self.bypass.store(bypass, Ordering::Relaxed);
    }

    pub fn bypassed(&self) -> bool {
        self.bypass.load(Ordering::Relaxed)
    }
}

pub struct PluginNode {
    processor: Box<dyn PluginProcessor>,
    incoming: Receiver<(u32, f64)>,
    outgoing: Sender<(u32, f64)>,
    changes: Vec<(u32, f64)>,
    bypass: Arc<AtomicBool>,
    automation: Arc<[AutomationSlot]>,
    sent: [u32; AUTOMATION_SLOTS],
    midi: Option<MidiPlayer>,
    /// Layered on the track's lead instrument (see `Insert::with_layer`).
    layer: Option<MidiLayer>,
    under: [Vec<f32>; 2],
    keyed: Arc<AtomicBool>,
    /// Copy of the key inputs for the block.
    key: [Vec<f32>; 2],
    levels: Levels,
}

impl std::fmt::Debug for PluginNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginNode").finish_non_exhaustive()
    }
}

impl PluginNode {
    pub fn new(processor: Box<dyn PluginProcessor>) -> (Self, PluginControls) {
        let (to_plugin, incoming) = channel(QUEUE);
        let (outgoing, from_plugin) = channel(QUEUE);
        let bypass = Arc::new(AtomicBool::new(false));
        let automation: Arc<[AutomationSlot]> = (0..AUTOMATION_SLOTS)
            .map(|_| AutomationSlot {
                id: AtomicU32::new(0),
                value: SharedParam::new(0.0),
                active: AtomicBool::new(false),
            })
            .collect();
        let (levels, level_controls) = Levels::new();
        let keyed = Arc::new(AtomicBool::new(false));
        (
            Self {
                processor,
                incoming,
                outgoing,
                changes: Vec::with_capacity(MAX_CHANGES),
                bypass: Arc::clone(&bypass),
                automation: Arc::clone(&automation),
                sent: [f32::NAN.to_bits(); AUTOMATION_SLOTS],
                midi: None,
                layer: None,
                under: [Vec::new(), Vec::new()],
                keyed: Arc::clone(&keyed),
                key: [Vec::new(), Vec::new()],
                levels,
            },
            PluginControls {
                to_plugin,
                from_plugin,
                bypass,
                automation,
                levels: level_controls,
                keyed,
            },
        )
    }
}

impl PluginNode {
    /// Gives the plugin the track's MIDI (instrument slot).
    pub fn with_midi(mut self, midi: Option<MidiPlayer>) -> Self {
        self.midi = midi;
        self
    }

    /// Layers the plugin on the track's lead instrument: it plays the lead's notes, over the
    /// sound it is given.
    pub fn with_layer(mut self, layer: Option<MidiLayer>) -> Self {
        self.layer = layer;
        self
    }
}

/// The engine's transport as plugins are given it.
fn transport(t: &crate::TransportInfo) -> audiostrada_plugins::Transport {
    let bar = &t.bar;
    // Quarter notes since the bar started.
    let into_bar = (f64::from(bar.beat.max(1) - 1) + bar.fraction) * bar.unit;
    let bar_start = (t.beat - into_bar).max(0.0);
    let bar_length = f64::from(bar.beats_per_bar.max(1)) * bar.unit;
    audiostrada_plugins::Transport {
        playing: t.playing,
        samples: t.position as i64,
        beats: t.beat,
        bar_start,
        bar: (bar_start / bar_length.max(1e-9)).round() as i32,
        tempo: t.bpm,
        numerator: bar.beats_per_bar.max(1),
        denominator: (4.0 / bar.unit.max(1e-9)).round().max(1.0) as u32,
    }
}

impl Processor for PluginNode {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.levels.prepare(sample_rate);
        self.under = [vec![0.0; max_frames], vec![0.0; max_frames]];
        self.key = [vec![0.0; max_frames], vec![0.0; max_frames]];
    }

    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        for ch in 0..b.input_channels().min(b.output_channels()) {
            let (src, dst) = b.io(ch, ch);
            dst.copy_from_slice(src);
        }
        self.changes.clear();
        while self.changes.len() < MAX_CHANGES {
            match self.incoming.try_recv() {
                Some(change) => self.changes.push(change),
                None => break,
            }
        }
        for (slot, sent) in self.automation.iter().zip(&mut self.sent) {
            if slot.active.load(Ordering::Relaxed) && self.changes.len() < MAX_CHANGES {
                let value = slot.value.get();
                if value.to_bits() != *sent {
                    *sent = value.to_bits();
                    self.changes
                        .push((slot.id.load(Ordering::Relaxed), f64::from(value)));
                }
            }
        }
        if b.output_channels() < 2 {
            return;
        }
        let frames = b.frames();
        let keyed = self.keyed.load(Ordering::Relaxed)
            && b.input_channels() >= 4
            && self.key[0].len() >= frames;
        if keyed {
            for (k, ch) in self.key.iter_mut().zip([2, 3]) {
                k[..frames].copy_from_slice(&b.input(ch)[..frames]);
            }
        }
        let (left, right) = b.output_pair(0, 1);
        if self.bypass.load(Ordering::Relaxed) {
            // A lead bypassed still hands its notes to the instruments layered on it.
            if let Some(midi) = &mut self.midi {
                midi.collect(ctx.transport, left.len());
            }
            if let Some(layer) = &mut self.layer {
                layer.collect();
            }
            self.levels.bypassed(left, right);
            return;
        }
        self.levels.input(left, right);
        let n = left.len();
        let layered = self.layer.is_some() && self.under[0].len() >= n;
        if layered {
            self.under[0][..n].copy_from_slice(left);
            self.under[1][..n].copy_from_slice(right);
            left.fill(0.0);
            right.fill(0.0);
        }
        let events: &[audiostrada_midi::MidiEvent] = match (&mut self.midi, &mut self.layer) {
            (Some(midi), _) => midi.collect(ctx.transport, n),
            (None, Some(layer)) => layer.collect(),
            (None, None) => &[],
        };
        self.processor.set_transport(&transport(ctx.transport));
        let outgoing = &mut self.outgoing;
        let mut report = |id, value| {
            let _ = outgoing.try_send((id, value));
        };
        if keyed {
            let key = (&self.key[0][..n], &self.key[1][..n]);
            self.processor
                .process_keyed(left, right, key, &self.changes, events, &mut report);
        } else {
            self.processor
                .process(left, right, &self.changes, events, &mut report);
        }
        if layered {
            for (s, u) in left.iter_mut().zip(&self.under[0]) {
                *s += u;
            }
            for (s, u) in right.iter_mut().zip(&self.under[1]) {
                *s += u;
            }
        }
        // A plugin's broken values must not stick in the effects after it.
        if left.iter().chain(right.iter()).any(|s| !s.is_finite()) {
            left.fill(0.0);
            right.fill(0.0);
        }
        self.levels.output(left, right);
    }

    fn reset(&mut self) {
        self.processor.reset();
        self.levels.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BarPosition, TransportInfo};

    #[test]
    fn plugins_are_told_where_the_song_is() {
        // Third beat and a half of the second bar of 4/4, at 96 BPM, playing.
        let info = TransportInfo {
            playing: true,
            position: 192_000,
            sample_rate: 48_000,
            beat: 6.5,
            bpm: 96.0,
            bar: BarPosition {
                beat: 3,
                fraction: 0.5,
                beats_per_bar: 4,
                unit: 1.0,
            },
        };
        let t = transport(&info);
        assert!(t.playing);
        assert_eq!((t.samples, t.beats, t.tempo), (192_000, 6.5, 96.0));
        assert_eq!((t.bar_start, t.bar), (4.0, 1));
        assert_eq!((t.numerator, t.denominator), (4, 4));
        // 6/8: beats are eighths, half a quarter note each.
        let info = TransportInfo {
            beat: 4.5,
            bar: BarPosition {
                beat: 4,
                fraction: 0.0,
                beats_per_bar: 6,
                unit: 0.5,
            },
            ..info
        };
        let t = transport(&info);
        assert_eq!((t.bar_start, t.bar), (3.0, 1));
        assert_eq!((t.numerator, t.denominator), (6, 8));
    }
}
