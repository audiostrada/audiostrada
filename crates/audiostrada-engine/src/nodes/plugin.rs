//! Insert slot running an external plugin. Parameter changes travel through wait-free queues in
//! both directions.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use audiostrada_plugins::PluginProcessor;
use audiostrada_rt::{Receiver, Sender, channel};

use crate::nodes::MidiPlayer;
use crate::nodes::levels::{LevelControls, Levels};
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
}

impl PluginControls {
    pub fn set_bypass(&self, bypass: bool) {
        self.bypass.store(bypass, Ordering::Relaxed);
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
                levels,
            },
            PluginControls {
                to_plugin,
                from_plugin,
                bypass,
                automation,
                levels: level_controls,
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
}

impl Processor for PluginNode {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.levels.prepare(sample_rate);
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
        let (left, right) = b.output_pair(0, 1);
        if self.bypass.load(Ordering::Relaxed) {
            self.levels.bypassed(left, right);
            return;
        }
        self.levels.input(left, right);
        let n = left.len();
        let events: &[audiostrada_midi::MidiEvent] = match &mut self.midi {
            Some(midi) => midi.collect(ctx.transport, n),
            None => &[],
        };
        let outgoing = &mut self.outgoing;
        self.processor
            .process(left, right, &self.changes, events, &mut |id, value| {
                let _ = outgoing.try_send((id, value));
            });
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
