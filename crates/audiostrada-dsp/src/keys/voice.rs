//! The keyboard side of the electric pianos: which voice plays which key, the sustain pedal,
//! velocity curves, tuning and pitch bend.

use crate::MidiEvent;

/// Voices of each electric piano.
pub(crate) const VOICES: usize = 64;

/// Pitch bend range, in semitones.
const BEND_RANGE: f32 = 2.0;

pub(crate) const CURVES: [&str; 3] = ["Soft", "Normal", "Hard"];

/// What a voice is doing, for the allocator.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Slot {
    pub key: u8,
    pub channel: u8,
    pub age: u64,
    /// Sounding (struck and not yet died away).
    pub on: bool,
    /// The key is held down.
    pub down: bool,
    /// Released while the pedal was down: damped when it lifts.
    pub sustained: bool,
}

/// What the instrument has to do after a MIDI message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Change {
    /// Voice `voice` strikes `key`; `again` when it was still sounding that key.
    Strike {
        voice: usize,
        key: u8,
        velocity: u8,
        again: bool,
    },
    /// The damper falls on voice `voice`.
    Damp(usize),
    /// Every voice stops at once.
    Silence,
}

#[derive(Clone, Debug)]
pub(crate) struct Keyboard {
    pub slots: [Slot; VOICES],
    clock: u64,
    sustain: [bool; 16],
    bend: [f32; 16],
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            slots: [Slot::default(); VOICES],
            clock: 0,
            sustain: [false; 16],
            bend: [0.0; 16],
        }
    }
}

impl Keyboard {
    pub fn midi(&mut self, event: &MidiEvent, mut apply: impl FnMut(Change)) {
        let channel = event.channel();
        let c = usize::from(channel);
        if let Some((key, velocity)) = event.as_note_on() {
            self.clock += 1;
            let same = self
                .slots
                .iter()
                .position(|s| s.on && s.key == key && s.channel == channel);
            let voice = same.unwrap_or_else(|| self.free());
            let slot = &mut self.slots[voice];
            *slot = Slot {
                key,
                channel,
                age: self.clock,
                on: true,
                down: true,
                sustained: false,
            };
            apply(Change::Strike {
                voice,
                key,
                velocity,
                again: same.is_some(),
            });
        } else if let Some(key) = event.as_note_off() {
            let pedal = self.sustain[c];
            for (i, s) in self.slots.iter_mut().enumerate() {
                if s.on && s.down && s.key == key && s.channel == channel {
                    s.down = false;
                    if pedal {
                        s.sustained = true;
                    } else {
                        apply(Change::Damp(i));
                    }
                }
            }
        } else if let Some((cc, value)) = event.as_control() {
            match cc {
                64 => self.pedal(channel, value >= 64, &mut apply),
                121 => {
                    self.bend[c] = 0.0;
                    self.pedal(channel, false, &mut apply);
                }
                120 => {
                    for s in &mut self.slots {
                        *s = Slot::default();
                    }
                    self.sustain = [false; 16];
                    apply(Change::Silence);
                }
                123 => {
                    for (i, s) in self.slots.iter_mut().enumerate() {
                        if s.on && (s.down || s.sustained) {
                            s.down = false;
                            s.sustained = false;
                            apply(Change::Damp(i));
                        }
                    }
                }
                _ => {}
            }
        } else if event.data[0] & 0xF0 == 0xE0 {
            let value = u16::from(event.data[2]) << 7 | u16::from(event.data[1]);
            self.bend[c] = ((f32::from(value) - 8192.0) / 8192.0).clamp(-1.0, 1.0);
        }
    }

    fn pedal(&mut self, channel: u8, down: bool, apply: &mut impl FnMut(Change)) {
        self.sustain[usize::from(channel)] = down;
        if down {
            return;
        }
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.on && s.sustained && s.channel == channel {
                s.sustained = false;
                apply(Change::Damp(i));
            }
        }
    }

    /// A silent voice, else the oldest one ringing after its key, else the oldest.
    fn free(&self) -> usize {
        let oldest = |keep: &dyn Fn(&Slot) -> bool| {
            (0..VOICES)
                .filter(|&i| keep(&self.slots[i]))
                .min_by_key(|&i| self.slots[i].age)
        };
        self.slots
            .iter()
            .position(|s| !s.on)
            .or_else(|| oldest(&|s| !s.down && !s.sustained))
            .or_else(|| oldest(&|s| !s.down))
            .or_else(|| oldest(&|_| true))
            .unwrap_or(0)
    }

    /// The voice has died away.
    pub fn finish(&mut self, voice: usize) {
        self.slots[voice] = Slot::default();
    }

    /// Pitch bend of `channel`, as a frequency ratio.
    pub fn bend(&self, channel: u8) -> f32 {
        2f32.powf(self.bend[usize::from(channel & 15)] * BEND_RANGE / 12.0)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A key's velocity on a curve, from 0 to 1.
pub(crate) fn velocity(velocity: u8, curve: f32) -> f32 {
    let v = f32::from(velocity.min(127)) / 127.0;
    match curve.round() as i32 {
        0 => v.powf(0.6),
        2 => v.powf(1.7),
        _ => v,
    }
}

/// Frequency of `key` with A4 at `a4` Hz, the octaves stretched by `stretch` (0 to 1): up to
/// 20 cents at the ends of the keyboard, as tuners stretch pianos.
pub(crate) fn frequency(key: f32, a4: f32, stretch: f32) -> f32 {
    let d = (key - 69.0) / 40.0;
    let cents = stretch * 20.0 * d * d * d;
    a4 * 2f32.powf((key - 69.0) / 12.0 + cents / 1200.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(keys: &mut Keyboard, event: MidiEvent) -> Vec<Change> {
        let mut out = Vec::new();
        keys.midi(&event, |c| out.push(c));
        out
    }

    #[test]
    fn the_pedal_holds_released_keys_until_it_lifts() {
        let mut keys = Keyboard::default();
        changes(&mut keys, MidiEvent::control(0, 0, 64, 127));
        changes(&mut keys, MidiEvent::note_on(0, 0, 60, 100));
        assert!(changes(&mut keys, MidiEvent::note_off(0, 0, 60)).is_empty());
        assert_eq!(
            changes(&mut keys, MidiEvent::control(0, 0, 64, 0)),
            [Change::Damp(0)]
        );
    }

    #[test]
    fn a_key_struck_again_keeps_its_voice_and_the_oldest_released_is_taken_first() {
        let mut keys = Keyboard::default();
        for key in 0..VOICES as u8 {
            changes(&mut keys, MidiEvent::note_on(0, 0, key, 100));
        }
        changes(&mut keys, MidiEvent::note_off(0, 0, 5));
        let again = changes(&mut keys, MidiEvent::note_on(0, 0, 3, 90));
        assert!(matches!(
            again[0],
            Change::Strike {
                voice: 3,
                again: true,
                ..
            }
        ));
        let stolen = changes(&mut keys, MidiEvent::note_on(0, 0, 100, 90));
        assert!(matches!(
            stolen[0],
            Change::Strike {
                voice: 5,
                again: false,
                ..
            }
        ));
    }

    #[test]
    fn tuning_follows_a4_and_the_stretch() {
        assert!((frequency(69.0, 440.0, 1.0) - 440.0).abs() < 1e-3);
        assert!((frequency(81.0, 442.0, 0.0) - 884.0).abs() < 1e-2);
        assert!(frequency(108.0, 440.0, 1.0) > frequency(108.0, 440.0, 0.0));
        assert!(frequency(21.0, 440.0, 1.0) < frequency(21.0, 440.0, 0.0));
        assert!(velocity(64, 0.0) > velocity(64, 1.0) && velocity(64, 1.0) > velocity(64, 2.0));
    }
}
