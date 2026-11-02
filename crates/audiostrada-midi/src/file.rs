//! Standard MIDI files: import (times in seconds, following the file's tempo changes) and export
//! (times in beats, one track per part).

use midly::num::{u4, u7, u14, u15, u24, u28};
use midly::{
    Format, Header, MetaMessage, MidiMessage, PitchBend, Smf, Timing, TrackEvent, TrackEventKind,
};

/// A note with times in seconds (import) or beats (export).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileNote {
    pub start: f64,
    pub length: f64,
    pub key: u8,
    pub velocity: u8,
    pub channel: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileTrack {
    pub name: String,
    pub notes: Vec<FileNote>,
    /// Controller, pitch bend and channel pressure messages, timed like the notes.
    pub controls: Vec<(f64, [u8; 3])>,
}

const EXPORT_PPQ: u16 = 960;

/// Reads the tracks with notes of a `.mid` file; times are in seconds.
pub fn import(bytes: &[u8]) -> Result<Vec<FileTrack>, String> {
    let smf = Smf::parse(bytes).map_err(|e| e.to_string())?;
    // Tempo changes (absolute tick, microseconds per quarter) from every track.
    let mut tempos: Vec<(u64, u32)> = Vec::new();
    for track in &smf.tracks {
        let mut tick = 0u64;
        for event in track {
            tick += u64::from(event.delta.as_int());
            if let TrackEventKind::Meta(MetaMessage::Tempo(t)) = event.kind {
                tempos.push((tick, t.as_int()));
            }
        }
    }
    tempos.sort_by_key(|t| t.0);
    let seconds = |tick: u64| -> f64 {
        match smf.header.timing {
            Timing::Metrical(ppq) => {
                let ppq = f64::from(ppq.as_int().max(1));
                let (mut time, mut last, mut tempo) = (0.0, 0u64, 500_000u32);
                for &(at, t) in tempos.iter().take_while(|t| t.0 <= tick) {
                    time += (at - last) as f64 * f64::from(tempo) / 1e6 / ppq;
                    last = at;
                    tempo = t;
                }
                time + (tick - last) as f64 * f64::from(tempo) / 1e6 / ppq
            }
            Timing::Timecode(fps, sub) => {
                tick as f64 / (f64::from(fps.as_f32()) * f64::from(sub.max(1)))
            }
        }
    };
    let mut out = Vec::new();
    for (index, track) in smf.tracks.iter().enumerate() {
        let mut name = format!("MIDI {}", index + 1);
        let mut tick = 0u64;
        let mut open: Vec<(u8, u8, u64, u8)> = Vec::new();
        let mut notes = Vec::new();
        let mut controls = Vec::new();
        for event in track {
            tick += u64::from(event.delta.as_int());
            match event.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(raw)) if !raw.is_empty() => {
                    name = String::from_utf8_lossy(raw).trim().to_owned();
                }
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    let (key, velocity, on) = match message {
                        MidiMessage::NoteOn { key, vel } => {
                            (key.as_int(), vel.as_int(), vel.as_int() > 0)
                        }
                        MidiMessage::NoteOff { key, .. } => (key.as_int(), 0, false),
                        MidiMessage::Controller { controller, value } => {
                            let data = [0xB0 | ch, controller.as_int(), value.as_int()];
                            controls.push((seconds(tick), data));
                            continue;
                        }
                        MidiMessage::PitchBend { bend } => {
                            let v = bend.0.as_int();
                            let data = [0xE0 | ch, (v & 0x7F) as u8, (v >> 7) as u8];
                            controls.push((seconds(tick), data));
                            continue;
                        }
                        MidiMessage::ChannelAftertouch { vel } => {
                            controls.push((seconds(tick), [0xD0 | ch, vel.as_int(), 0]));
                            continue;
                        }
                        _ => continue,
                    };
                    if on {
                        open.push((ch, key, tick, velocity));
                    } else if let Some(i) = open.iter().position(|o| o.0 == ch && o.1 == key) {
                        let (_, _, start, velocity) = open.remove(i);
                        let begin = seconds(start);
                        notes.push(FileNote {
                            start: begin,
                            length: (seconds(tick) - begin).max(0.001),
                            key,
                            velocity,
                            channel: ch,
                        });
                    }
                }
                _ => {}
            }
        }
        // Notes never ended last until the end of the track.
        for (ch, key, start, velocity) in open {
            let begin = seconds(start);
            notes.push(FileNote {
                start: begin,
                length: (seconds(tick) - begin).max(0.001),
                key,
                velocity,
                channel: ch,
            });
        }
        if !notes.is_empty() || !controls.is_empty() {
            notes.sort_by(|a, b| a.start.total_cmp(&b.start));
            out.push(FileTrack {
                name,
                notes,
                controls,
            });
        }
    }
    Ok(out)
}

/// Writes a format 1 file at `bpm`; note times are in beats.
pub fn export(tracks: &[FileTrack], bpm: f64) -> Result<Vec<u8>, String> {
    let ticks = |beats: f64| (beats.max(0.0) * f64::from(EXPORT_PPQ)).round() as u64;
    let mut smf = Smf::new(Header::new(
        Format::Parallel,
        Timing::Metrical(u15::new(EXPORT_PPQ)),
    ));
    // Microseconds per quarter, within the 24 bits of the tempo event (about 3.6 BPM or more).
    let tempo = (60_000_000.0 / bpm.max(1.0))
        .round()
        .min(f64::from(0xFF_FFFF)) as u32;
    smf.tracks.push(vec![
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(tempo))),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ]);
    for track in tracks {
        // (tick, order, event): note offs before note ons at the same tick.
        let mut events: Vec<(u64, u8, TrackEventKind<'_>)> = vec![(
            0,
            0,
            TrackEventKind::Meta(MetaMessage::TrackName(track.name.as_bytes())),
        )];
        for n in &track.notes {
            let (channel, key) = (u4::new(n.channel & 15), u7::new(n.key & 127));
            events.push((
                ticks(n.start),
                2,
                TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOn {
                        key,
                        vel: u7::new(n.velocity.clamp(1, 127)),
                    },
                },
            ));
            events.push((
                ticks(n.start + n.length).max(ticks(n.start) + 1),
                1,
                TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOff {
                        key,
                        vel: u7::new(0),
                    },
                },
            ));
        }
        for &(time, data) in &track.controls {
            let channel = u4::new(data[0] & 15);
            let message = match data[0] & 0xF0 {
                0xB0 => MidiMessage::Controller {
                    controller: u7::new(data[1] & 127),
                    value: u7::new(data[2] & 127),
                },
                0xE0 => MidiMessage::PitchBend {
                    bend: PitchBend(u14::new(
                        u16::from(data[2] & 127) << 7 | u16::from(data[1] & 127),
                    )),
                },
                0xD0 => MidiMessage::ChannelAftertouch {
                    vel: u7::new(data[1] & 127),
                },
                _ => continue,
            };
            events.push((ticks(time), 0, TrackEventKind::Midi { channel, message }));
        }
        events.sort_by_key(|e| (e.0, e.1));
        let mut last = 0;
        let mut out: Vec<TrackEvent<'_>> = events
            .into_iter()
            .map(|(tick, _, kind)| {
                let delta = u28::new((tick - last) as u32);
                last = tick;
                TrackEvent { delta, kind }
            })
            .collect();
        out.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf.tracks.push(out);
    }
    let mut bytes = Vec::new();
    smf.write_std(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_then_import_keeps_the_notes() {
        let note = |start, length, key| FileNote {
            start,
            length,
            key,
            velocity: 90,
            channel: 0,
        };
        let track = FileTrack {
            name: "Lead".into(),
            notes: vec![note(0.0, 1.0, 60), note(1.0, 0.5, 64), note(4.0, 2.0, 67)],
            controls: vec![(1.0, [0xB0, 1, 100]), (2.0, [0xE0, 0, 96])],
        };
        let bytes = export(std::slice::from_ref(&track), 120.0).unwrap();
        let tracks = import(&bytes).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name, "Lead");
        // At 120 BPM a beat is half a second.
        let starts: Vec<f64> = tracks[0].notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, vec![0.0, 0.5, 2.0]);
        assert!((tracks[0].notes[2].length - 1.0).abs() < 1e-9);
        assert_eq!(
            tracks[0].controls,
            vec![(0.5, [0xB0, 1, 100]), (1.0, [0xE0, 0, 96])]
        );
        assert!(import(b"not a midi file").is_err());
    }

    fn event(delta: u32, kind: TrackEventKind<'static>) -> TrackEvent<'static> {
        TrackEvent {
            delta: u28::new(delta),
            kind,
        }
    }

    fn midi(channel: u8, message: MidiMessage) -> TrackEventKind<'static> {
        TrackEventKind::Midi {
            channel: u4::new(channel),
            message,
        }
    }

    fn on(key: u8, vel: u8) -> MidiMessage {
        MidiMessage::NoteOn {
            key: u7::new(key),
            vel: u7::new(vel),
        }
    }

    fn write(smf: &Smf<'_>) -> Vec<u8> {
        let mut bytes = Vec::new();
        smf.write_std(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn import_follows_tempo_changes() {
        // 480 PPQ: 120 BPM for the first beat, then 60 BPM.
        let mut smf = Smf::new(Header::new(
            Format::Parallel,
            Timing::Metrical(u15::new(480)),
        ));
        smf.tracks.push(vec![
            event(
                0,
                TrackEventKind::Meta(MetaMessage::Tempo(u24::new(500_000))),
            ),
            event(
                480,
                TrackEventKind::Meta(MetaMessage::Tempo(u24::new(1_000_000))),
            ),
        ]);
        smf.tracks.push(vec![
            event(0, TrackEventKind::Meta(MetaMessage::TrackName(b" Bass "))),
            event(0, midi(2, on(40, 80))),
            // A note on with velocity zero ends the note.
            event(960, midi(2, on(40, 0))),
            event(
                0,
                midi(2, MidiMessage::ChannelAftertouch { vel: u7::new(33) }),
            ),
            // Never ended: lasts until the end of the track, one second later.
            event(0, midi(2, on(41, 80))),
            event(480, TrackEventKind::Meta(MetaMessage::EndOfTrack)),
        ]);
        let tracks = import(&write(&smf)).unwrap();
        assert_eq!(tracks.len(), 1, "the tempo track has no notes");
        assert_eq!(tracks[0].name, "Bass");
        assert_eq!(
            tracks[0].notes,
            vec![
                FileNote {
                    start: 0.0,
                    length: 1.5,
                    key: 40,
                    velocity: 80,
                    channel: 2,
                },
                FileNote {
                    start: 1.5,
                    length: 1.0,
                    key: 41,
                    velocity: 80,
                    channel: 2,
                },
            ]
        );
        assert_eq!(tracks[0].controls, vec![(1.5, [0xD2, 33, 0])]);
    }

    #[test]
    fn import_reads_timecode_files() {
        // 25 frames per second, 40 ticks per frame: 1000 ticks a second.
        let mut smf = Smf::new(Header::new(
            Format::SingleTrack,
            Timing::Timecode(midly::Fps::Fps25, 40),
        ));
        smf.tracks.push(vec![
            event(500, midi(0, on(60, 100))),
            event(
                250,
                midi(
                    0,
                    MidiMessage::NoteOff {
                        key: u7::new(60),
                        vel: u7::new(0),
                    },
                ),
            ),
        ]);
        let tracks = import(&write(&smf)).unwrap();
        assert_eq!(tracks[0].name, "MIDI 1");
        assert_eq!(tracks[0].notes[0].start, 0.5);
        assert_eq!(tracks[0].notes[0].length, 0.25);
    }

    #[test]
    fn export_orders_and_clamps_events() {
        let track = FileTrack {
            name: "Keys".into(),
            notes: vec![
                FileNote {
                    start: 1.0,
                    length: 0.0,
                    key: 60,
                    velocity: 0,
                    channel: 17,
                },
                FileNote {
                    start: 0.0,
                    length: 1.0,
                    key: 60,
                    velocity: 100,
                    channel: 1,
                },
            ],
            controls: vec![(0.0, [0xC0, 5, 0]), (0.5, [0xB1, 7, 64])],
        };
        let bytes = export(&[track], 2.0).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        assert_eq!(smf.tracks.len(), 2);
        let TrackEventKind::Meta(MetaMessage::Tempo(tempo)) = smf.tracks[0][0].kind else {
            panic!("expected a tempo");
        };
        assert_eq!(tempo.as_int(), 0xFF_FFFF, "very slow tempos are clamped");
        let kinds: Vec<_> = smf.tracks[1]
            .iter()
            .map(|e| (e.delta.as_int(), e.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (0, TrackEventKind::Meta(MetaMessage::TrackName(b"Keys"))),
                (0, midi(1, on(60, 100))),
                (
                    480,
                    midi(
                        1,
                        MidiMessage::Controller {
                            controller: u7::new(7),
                            value: u7::new(64),
                        }
                    )
                ),
                // The note off comes before the note on of the same tick.
                (
                    480,
                    midi(
                        1,
                        MidiMessage::NoteOff {
                            key: u7::new(60),
                            vel: u7::new(0),
                        }
                    )
                ),
                (0, midi(1, on(60, 1))),
                (
                    1,
                    midi(
                        1,
                        MidiMessage::NoteOff {
                            key: u7::new(60),
                            vel: u7::new(0),
                        }
                    )
                ),
                (0, TrackEventKind::Meta(MetaMessage::EndOfTrack)),
            ]
        );
    }
}
