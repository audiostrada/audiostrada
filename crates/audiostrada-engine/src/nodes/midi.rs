//! MIDI playback for an instrument insert: notes of the track's MIDI clips, plus live input when
//! the track is armed and notes played on the editor's keyboard, turned into events for each
//! block. The clips' notes are released on stops and jumps (a loop's end); notes played live
//! keep sounding until they are let go.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

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
    /// Panic: every note off and every sound off, on every channel.
    pub fn panic(&self) {
        for channel in 0..16u8 {
            self.keys.push([0xB0 | channel, 123, 0], 0);
            self.keys.push([0xB0 | channel, 120, 0], 0);
        }
    }

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

/// The notes a track's lead instrument played in the last block, for the instruments layered
/// on it (processed after it in the same block). Wait-free: events packed in atomics.
#[derive(Debug)]
pub struct MidiShare {
    /// Raised at each block published.
    block: AtomicU64,
    count: AtomicU32,
    events: Box<[AtomicU64]>,
}

impl MidiShare {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            block: AtomicU64::new(0),
            count: AtomicU32::new(0),
            events: (0..MAX_EVENTS).map(|_| AtomicU64::new(0)).collect(),
        })
    }

    fn publish(&self, events: &[MidiEvent]) {
        let n = events.len().min(self.events.len());
        for (slot, e) in self.events.iter().zip(&events[..n]) {
            let packed = (u64::from(e.time) << 32)
                | (u64::from(e.data[0]) << 16)
                | (u64::from(e.data[1]) << 8)
                | u64::from(e.data[2]);
            slot.store(packed, Ordering::Relaxed);
        }
        self.count.store(n as u32, Ordering::Relaxed);
        self.block.fetch_add(1, Ordering::Release);
    }
}

/// An instrument layered on a track's lead: it plays the notes the lead played this block.
#[derive(Debug)]
pub struct MidiLayer {
    share: Arc<MidiShare>,
    seen: u64,
    events: Vec<MidiEvent>,
}

impl MidiLayer {
    pub fn new(share: Arc<MidiShare>) -> Self {
        let seen = share.block.load(Ordering::Acquire);
        Self {
            share,
            seen,
            events: Vec::with_capacity(MAX_EVENTS),
        }
    }

    /// The lead's notes of this block; none when it published nothing since the last call.
    pub fn collect(&mut self) -> &[MidiEvent] {
        self.events.clear();
        let block = self.share.block.load(Ordering::Acquire);
        if block != self.seen {
            self.seen = block;
            let n = (self.share.count.load(Ordering::Relaxed) as usize).min(MAX_EVENTS);
            for slot in &self.share.events[..n] {
                let v = slot.load(Ordering::Relaxed);
                self.events.push(MidiEvent {
                    time: (v >> 32) as u32,
                    data: [(v >> 16) as u8, (v >> 8) as u8, v as u8],
                });
            }
        }
        &self.events
    }
}

pub struct MidiPlayer {
    /// Where the notes of each block go for the instruments layered on this one.
    share: Option<Arc<MidiShare>>,
    sequence: Box<Sequence>,
    sequences: Receiver<Box<Sequence>>,
    garbage: Sender<Box<Sequence>>,
    live: LiveReader,
    armed: Arc<AtomicBool>,
    keys: LiveReader,
    /// Sounding notes of the clips per channel.
    active: [u128; 16],
    /// Notes held live (MIDI input, the editors' keyboards) per channel.
    held: [u128; 16],
    /// Keys of the MIDI input pressed while armed: their release still comes once disarmed.
    input_held: [u128; 16],
    expected: Option<u64>,
    was_playing: bool,
    events: Vec<MidiEvent>,
    /// Draws whether the notes with a probability play (xorshift, no allocation).
    seed: u32,
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
                share: None,
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
                held: [0; 16],
                input_held: [0; 16],
                expected: None,
                was_playing: false,
                events: Vec::with_capacity(MAX_EVENTS),
                seed: 0x9E37_79B9,
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

    /// Shares each block's notes with the instruments layered on this one.
    pub fn with_share(mut self, share: Arc<MidiShare>) -> Self {
        self.share = Some(share);
        self
    }

    fn push(&mut self, event: MidiEvent) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(event);
        }
    }

    /// Whether a note with this chance (percent) plays this time.
    fn plays(&mut self, probability: u8) -> bool {
        if probability >= 100 {
            return true;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 100) < u32::from(probability)
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
                        if note.muted || !self.plays(note.probability) {
                            continue;
                        }
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
        let mut incoming = [MidiEvent::default(); 256];
        let mut n = 0;
        let mut take = |data| {
            if n < incoming.len() {
                incoming[n] = MidiEvent { time: 0, data };
                n += 1;
            }
        };
        let armed = self.armed.load(Ordering::Relaxed);
        let input_held = &mut self.input_held;
        self.live.drain(|data, _| {
            let event = MidiEvent { time: 0, data };
            let (channel, bit) = (usize::from(event.channel()), |key: u8| 1u128 << key);
            if let Some((key, _)) = event.as_note_on() {
                if armed {
                    input_held[channel] |= bit(key);
                    take(data);
                }
            } else if let Some(key) = event.as_note_off() {
                if armed || input_held[channel] & bit(key) != 0 {
                    input_held[channel] &= !bit(key);
                    take(data);
                }
            } else if armed {
                take(data);
            }
        });
        self.keys.drain(|data, _| take(data));
        for event in &incoming[..n] {
            let channel = usize::from(event.channel());
            if let Some((key, _)) = event.as_note_on() {
                self.held[channel] |= 1 << key;
            } else if let Some(key) = event.as_note_off() {
                self.held[channel] &= !(1 << key);
            } else if event
                .as_control()
                .is_some_and(|(cc, _)| cc == 120 || cc == 123)
            {
                self.held[channel] = 0;
                self.active[channel] = 0;
            }
            self.push(*event);
        }
        self.events.sort_by_key(|e| e.time);
        if let Some(share) = &self.share {
            share.publish(&self.events);
        }
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
            counting: false,
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
            ..Default::default()
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
    fn a_loop_end_releases_the_clips_notes_not_the_ones_played() {
        let live = audiostrada_midi::live::LiveMidi::default();
        let (mut player, controls) = MidiPlayer::new(live.reader());
        controls.keys.push([0x90, 48, 100], 0);
        player.collect(&transport(900, true), 100);
        // The loop wraps back to 0: nothing to release, the held note keeps sounding.
        let events = player.collect(&transport(0, true), 64).to_vec();
        assert!(events.is_empty(), "{events:?}");
        controls.panic();
        let events = player.collect(&transport(64, true), 64).to_vec();
        assert_eq!(events.len(), 32);
        assert!(events.iter().all(|e| e.as_control().is_some()));
    }

    #[test]
    fn muted_notes_stay_silent_and_chances_are_drawn() {
        let live = audiostrada_midi::live::LiveMidi::default();
        let (mut player, mut controls) = MidiPlayer::new(live.reader());
        let notes: Vec<Note> = (0..200)
            .map(|i| Note {
                start: i * 10,
                length: 5,
                key: 60,
                probability: if i == 0 { 100 } else { 50 },
                muted: i == 1,
                ..Default::default()
            })
            .collect();
        controls.set(Sequence {
            clips: vec![MidiClip {
                start: 0,
                offset: 0,
                length: 2000,
                notes: Arc::new(notes),
                controls: Arc::default(),
            }],
            ratio: 1.0,
        });
        let mut ons = Vec::new();
        for block in 0..32u64 {
            let events = player.collect(&transport(block * 64, true), 64).to_vec();
            ons.extend(
                events
                    .iter()
                    .filter(|e| e.as_note_on().is_some())
                    .map(|e| block * 64 + u64::from(e.time)),
            );
        }
        assert_eq!(ons.first(), Some(&0), "a sure note plays");
        assert!(!ons.contains(&10), "a muted note does not");
        assert!((60..140).contains(&ons.len()), "about half: {}", ons.len());
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

    #[test]
    fn a_key_held_when_the_track_is_disarmed_is_still_released() {
        let live = audiostrada_midi::live::LiveMidi::default();
        let (mut player, controls) = MidiPlayer::new(live.reader());
        controls.armed.store(true, Ordering::Relaxed);
        live.push([0x90, 60, 100], 0);
        player.collect(&transport(0, false), 64);
        controls.armed.store(false, Ordering::Relaxed);
        // Disarmed: new keys and controllers stay out, the held key's release comes in.
        live.push([0x90, 64, 100], 0);
        live.push([0xB0, 1, 90], 0);
        live.push([0x80, 64, 0], 0);
        live.push([0x80, 60, 0], 0);
        let events = player.collect(&transport(64, false), 64).to_vec();
        assert_eq!(events, vec![MidiEvent::note_off(0, 0, 60)]);
    }
}
