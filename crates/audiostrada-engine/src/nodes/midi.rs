//! MIDI playback for an instrument insert: notes of the track's MIDI clips, plus live input when
//! the track is armed and notes played on the editor's keyboard, turned into events for each
//! block. Notes are released on stops and jumps.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use audiostrada_core::{ControlPoint, Note};
use audiostrada_midi::MidiEvent;
use audiostrada_midi::live::{LiveMidi, LiveReader};
use audiostrada_rt::{Receiver, Sender, channel};

use crate::TransportInfo;

const MAX_EVENTS: usize = 1024;

/// A MIDI clip, in project samples.
#[derive(Clone, Debug, PartialEq)]
pub struct MidiClip {
    pub start: u64,
    pub offset: u64,
    pub length: u64,
    pub notes: Arc<Vec<Note>>,
    pub controls: Arc<Vec<ControlPoint>>,
}

/// Every MIDI clip of a track, with the engine/project rate ratio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sequence {
    pub clips: Vec<MidiClip>,
    pub ratio: f64,
}

/// UI side of a MIDI player.
#[derive(Debug)]
pub struct MidiControls {
    sequences: Sender<Box<Sequence>>,
    garbage: Receiver<Box<Sequence>>,
    pub armed: Arc<AtomicBool>,
    /// Notes played from the user interface, heard whether the track is armed or not.
    pub keys: LiveMidi,
    sent: Option<Sequence>,
}

impl MidiControls {
    /// Sends the clips if they changed.
    pub fn set(&mut self, sequence: Sequence) {
        while self.garbage.try_recv().is_some() {}
        if self.sent.as_ref() == Some(&sequence) {
            return;
        }
        if self.sequences.try_send(Box::new(sequence.clone())).is_ok() {
            self.sent = Some(sequence);
        }
    }
}

pub struct MidiPlayer {
    sequence: Box<Sequence>,
    sequences: Receiver<Box<Sequence>>,
    garbage: Sender<Box<Sequence>>,
    live: LiveReader,
    armed: Arc<AtomicBool>,
    keys: LiveReader,
    /// Sounding notes per channel.
    active: [u128; 16],
    expected: Option<u64>,
    was_playing: bool,
    events: Vec<MidiEvent>,
}

impl std::fmt::Debug for MidiPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MidiPlayer")
            .field("clips", &self.sequence.clips.len())
            .finish_non_exhaustive()
    }
}

impl MidiPlayer {
    pub fn new(live: LiveReader) -> (Self, MidiControls) {
        let (tx, sequences) = channel(16);
        let (garbage, garbage_rx) = channel(16);
        let armed = Arc::new(AtomicBool::new(false));
        let keys = LiveMidi::default();
        (
            Self {
                sequence: Box::new(Sequence {
                    clips: Vec::new(),
                    ratio: 1.0,
                }),
                sequences,
                garbage,
                live,
                armed: Arc::clone(&armed),
                keys: keys.reader(),
                active: [0; 16],
                expected: None,
                was_playing: false,
                events: Vec::with_capacity(MAX_EVENTS),
            },
            MidiControls {
                sequences: tx,
                garbage: garbage_rx,
                armed,
                keys,
                sent: None,
            },
        )
    }

    fn push(&mut self, event: MidiEvent) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(event);
        }
    }

    fn release_all(&mut self) {
        for channel in 0..16u8 {
            let mut bits = self.active[usize::from(channel)];
            while bits != 0 {
                let key = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                self.push(MidiEvent::note_off(0, channel, key));
            }
            self.active[usize::from(channel)] = 0;
        }
    }

    /// Events for a block of `frames` at the transport position, sorted by time.
    pub fn collect(&mut self, transport: &TransportInfo, frames: usize) -> &[MidiEvent] {
        while let Some(new) = self.sequences.try_recv() {
            let old = std::mem::replace(&mut self.sequence, new);
            if let Err(old) = self.garbage.try_send(old) {
                audiostrada_rt::permit_alloc(|| drop(old));
            }
        }
        self.events.clear();
        let position = transport.position;
        let jumped = transport.playing && self.was_playing && self.expected != Some(position);
        let started = transport.playing && (!self.was_playing || jumped);
        if jumped || (self.was_playing && !transport.playing) {
            self.release_all();
        }
        if transport.playing {
            let ratio = self.sequence.ratio.max(1e-9);
            let from = (position as f64 / ratio) as u64;
            let to = ((position + frames as u64) as f64 / ratio) as u64;
            let time = |t: u64| {
                (((t as f64 * ratio) as u64).saturating_sub(position) as usize)
                    .min(frames.saturating_sub(1)) as u32
            };
            for c in 0..self.sequence.clips.len() {
                let clip = &self.sequence.clips[c];
                let end = clip.start + clip.length;
                if clip.start >= to || end < from {
                    continue;
                }
                let origin = clip.start.saturating_sub(clip.offset);
                let notes = Arc::clone(&clip.notes);
                let controls = Arc::clone(&clip.controls);
                let start = clip.start;
                if started && clip.start <= from {
                    // Chase: the last value of each controller before the start position.
                    let mut last: [Option<ControlPoint>; 32] = [None; 32];
                    for p in controls.iter() {
                        let at = origin + p.time;
                        if at < start || at > from {
                            continue;
                        }
                        let slot = last
                            .iter()
                            .position(|l| {
                                l.is_some_and(|l| {
                                    l.controller == p.controller && l.channel == p.channel
                                })
                            })
                            .or_else(|| last.iter().position(Option::is_none));
                        if let Some(slot) = slot {
                            last[slot] = Some(*p);
                        }
                    }
                    for p in last.iter().flatten() {
                        let data = p.message();
                        self.push(MidiEvent { time: 0, data });
                    }
                }
                for p in controls.iter() {
                    let at = origin + p.time;
                    if at >= start && at < end && at >= from && at < to {
                        let data = p.message();
                        self.push(MidiEvent {
                            time: time(at),
                            data,
                        });
                    }
                }
                for note in notes.iter() {
                    let on = origin + note.start;
                    let off = (on + note.length).min(end);
                    let (ch, key) = (usize::from(note.channel & 15), note.key & 127);
                    if on >= start && on < end && on >= from && on < to {
                        self.push(MidiEvent::note_on(
                            time(on),
                            note.channel,
                            key,
                            note.velocity.max(1),
                        ));
                        self.active[ch] |= 1 << key;
                    }
                    if off >= from && off < to && self.active[ch] & (1 << key) != 0 && on < off {
                        self.push(MidiEvent::note_off(time(off), note.channel, key));
                        self.active[ch] &= !(1 << key);
                    }
                }
            }
            self.expected = Some(position + frames as u64);
        } else {
            self.expected = None;
        }
        self.was_playing = transport.playing;
        let mut incoming = [MidiEvent::default(); 64];
        let mut n = 0;
        let mut take = |data| {
            if n < incoming.len() {
                incoming[n] = MidiEvent { time: 0, data };
                n += 1;
            }
        };
        if self.armed.load(Ordering::Relaxed) {
            self.live.drain(|data, _| take(data));
        } else {
            self.live.drain(|_, _| {});
        }
        self.keys.drain(|data, _| take(data));
        for event in &incoming[..n] {
            if let Some((key, _)) = event.as_note_on() {
                self.active[usize::from(event.channel())] |= 1 << key;
            } else if let Some(key) = event.as_note_off() {
                self.active[usize::from(event.channel())] &= !(1 << key);
            }
            self.push(*event);
        }
        self.events.sort_by_key(|e| e.time);
        &self.events
    }
}

/// Sends a track's MIDI (clips, and live input while armed) to an output ring, for an
/// external instrument.
pub struct MidiOut {
    player: MidiPlayer,
    out: audiostrada_midi::live::LiveMidi,
}

impl std::fmt::Debug for MidiOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MidiOut").finish_non_exhaustive()
    }
}

impl MidiOut {
    pub fn new(player: MidiPlayer, out: audiostrada_midi::live::LiveMidi) -> Self {
        Self { player, out }
    }
}

impl crate::Processor for MidiOut {
    fn process(&mut self, ctx: &mut crate::ProcessContext<'_>) {
        ctx.buffers.clear_outputs();
        let frames = ctx.buffers.frames();
        for event in self.player.collect(ctx.transport, frames) {
            self.out.push(event.data, u64::from(event.time));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport(position: u64, playing: bool) -> TransportInfo {
        TransportInfo {
            playing,
            position,
            sample_rate: 48_000,
            beat: 0.0,
            bpm: 120.0,
            bar: Default::default(),
        }
    }

    #[test]
    fn notes_play_inside_their_clip_and_stop_on_jumps() {
        let live = audiostrada_midi::live::LiveMidi::default();
        let (mut player, mut controls) = MidiPlayer::new(live.reader());
        let note = |start, length, key| Note {
            start,
            length,
            key,
            velocity: 100,
            channel: 0,
        };
        controls.set(Sequence {
            clips: vec![MidiClip {
                start: 1000,
                offset: 0,
                length: 1000,
                notes: Arc::new(vec![note(10, 100, 60), note(900, 500, 64)]),
                controls: Arc::default(),
            }],
            ratio: 1.0,
        });
        let events = player.collect(&transport(960, true), 64).to_vec();
        assert_eq!(events, vec![MidiEvent::note_on(50, 0, 60, 100)]);
        let events = player.collect(&transport(1024, true), 128).to_vec();
        assert_eq!(events, vec![MidiEvent::note_off(86, 0, 60)]);
        let events = player.collect(&transport(1900, true), 64).to_vec();
        assert_eq!(events, vec![MidiEvent::note_on(0, 0, 64, 100)]);
        let events = player.collect(&transport(0, true), 64).to_vec();
        assert_eq!(events, vec![MidiEvent::note_off(0, 0, 64)], "jump releases");
        controls.armed.store(true, Ordering::Relaxed);
        live.push([0x90, 72, 80], 0);
        let events = player.collect(&transport(64, false), 64).to_vec();
        assert_eq!(events, vec![MidiEvent::note_on(0, 0, 72, 80)]);
    }

    #[test]
    fn keyboard_notes_play_without_arming() {
        let live = audiostrada_midi::live::LiveMidi::default();
        let (mut player, controls) = MidiPlayer::new(live.reader());
        live.push([0x90, 40, 90], 0);
        controls.keys.push([0x90, 60, 100], 0);
        let events = player.collect(&transport(0, false), 64).to_vec();
        assert_eq!(
            events,
            vec![MidiEvent::note_on(0, 0, 60, 100)],
            "live input waits for arming"
        );
        controls.keys.push([0x80, 60, 0], 0);
        let events = player.collect(&transport(64, false), 64).to_vec();
        assert_eq!(events, vec![MidiEvent::note_off(0, 0, 60)]);
    }

    #[test]
    fn plays_and_chases_controllers() {
        let live = audiostrada_midi::live::LiveMidi::default();
        let (mut player, mut controls) = MidiPlayer::new(live.reader());
        let point = |time, controller, value| ControlPoint {
            time,
            controller,
            value,
            channel: 0,
        };
        controls.set(Sequence {
            clips: vec![MidiClip {
                start: 1000,
                offset: 0,
                length: 4000,
                notes: Arc::default(),
                controls: Arc::new(vec![
                    point(100, 1, 0.5),
                    point(200, 1, 1.0),
                    point(300, audiostrada_core::model::PITCH_BEND, 0.5),
                    point(2000, 7, 0.0),
                ]),
            }],
            ratio: 1.0,
        });
        let events = player.collect(&transport(1000, true), 256).to_vec();
        assert_eq!(
            events.iter().map(|e| (e.time, e.data)).collect::<Vec<_>>(),
            vec![(100, [0xB0, 1, 64]), (200, [0xB0, 1, 127])]
        );
        // Starting later sends the last value of each controller first.
        let mut events = player.collect(&transport(2500, true), 64).to_vec();
        events.sort_by_key(|e| e.data);
        assert_eq!(
            events.iter().map(|e| (e.time, e.data)).collect::<Vec<_>>(),
            vec![(0, [0xB0, 1, 127]), (0, [0xE0, 0, 64])]
        );
        let events = player.collect(&transport(2564, true), 512).to_vec();
        assert_eq!(
            events.iter().map(|e| (e.time, e.data)).collect::<Vec<_>>(),
            vec![(436, [0xB0, 7, 0])]
        );
    }
}
