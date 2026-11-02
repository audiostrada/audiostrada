//! AudioStrada MIDI: events, a broadcast ring for live input, MIDI files and devices.

pub mod devices;
pub mod file;
pub mod live;

/// A short MIDI message at a frame offset within a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MidiEvent {
    pub time: u32,
    pub data: [u8; 3],
}

impl MidiEvent {
    pub fn note_on(time: u32, channel: u8, key: u8, velocity: u8) -> Self {
        Self {
            time,
            data: [0x90 | (channel & 15), key & 127, velocity & 127],
        }
    }

    pub fn note_off(time: u32, channel: u8, key: u8) -> Self {
        Self {
            time,
            data: [0x80 | (channel & 15), key & 127, 0],
        }
    }

    pub fn control(time: u32, channel: u8, controller: u8, value: u8) -> Self {
        Self {
            time,
            data: [0xB0 | (channel & 15), controller & 127, value & 127],
        }
    }

    pub fn channel(&self) -> u8 {
        self.data[0] & 15
    }

    /// Note on with a non-zero velocity: (key, velocity).
    pub fn as_note_on(&self) -> Option<(u8, u8)> {
        (self.data[0] & 0xF0 == 0x90 && self.data[2] > 0).then_some((self.data[1], self.data[2]))
    }

    /// Note off, or note on with velocity zero: the key.
    pub fn as_note_off(&self) -> Option<u8> {
        let status = self.data[0] & 0xF0;
        (status == 0x80 || (status == 0x90 && self.data[2] == 0)).then_some(self.data[1])
    }

    /// Control change: (controller, value).
    pub fn as_control(&self) -> Option<(u8, u8)> {
        (self.data[0] & 0xF0 == 0xB0).then_some((self.data[1], self.data[2]))
    }
}

/// Name of a MIDI key, e.g. 60 is "C4".
pub fn key_name(key: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(key % 12)],
        i32::from(key / 12) - 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        let on = MidiEvent::note_on(3, 1, 60, 100);
        assert_eq!(on.data, [0x91, 60, 100]);
        assert_eq!(on.as_note_on(), Some((60, 100)));
        assert_eq!(MidiEvent::note_on(0, 0, 60, 0).as_note_off(), Some(60));
        assert_eq!(MidiEvent::control(0, 0, 7, 99).as_control(), Some((7, 99)));
        assert_eq!(key_name(60), "C4");
        assert_eq!(key_name(69), "A4");
    }

    #[test]
    fn fields_are_masked_and_kinds_exclusive() {
        let off = MidiEvent::note_off(0, 0x1F, 0xBC);
        assert_eq!(off.data, [0x8F, 60, 0]);
        assert_eq!(off.channel(), 15);
        assert_eq!(off.as_note_off(), Some(60));
        assert_eq!(off.as_note_on(), None);
        assert_eq!(off.as_control(), None);
        let cc = MidiEvent::control(0, 2, 200, 255);
        assert_eq!(cc.data, [0xB2, 72, 127]);
        assert_eq!(cc.as_note_on(), None);
        assert_eq!(cc.as_note_off(), None);
        assert_eq!(MidiEvent::note_on(0, 0, 60, 1).as_note_off(), None);
        assert_eq!(key_name(0), "C-1");
        assert_eq!(key_name(127), "G9");
        assert_eq!(key_name(61), "C#4");
    }
}
