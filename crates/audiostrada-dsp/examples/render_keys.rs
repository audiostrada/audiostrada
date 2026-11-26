//! Renders a built-in instrument to a WAV file, for listening and for comparing it with what
//! `audiostrada measure-instrument` recorded from a reference, without building the whole
//! application.
//!
//!   cargo run --release -p audiostrada-dsp --example render_keys -- tines out.wav notes
//!   cargo run --release -p audiostrada-dsp --example render_keys -- ep80 out.wav chord 127 48,55,60,64,67,72
//!   cargo run --release -p audiostrada-dsp --example render_keys -- reed out.wav phrase --set reverb=0
//!
//! `notes` plays what `measure-instrument` plays: a key every fifth from 28 to 98 at velocities
//! 20, 45, 70, 95 and 120, each held 4 s then 2 s of silence. `chord` strikes the keys together
//! at one velocity, held 2 s; `phrase` plays the four chords of the listening phrase.
//! `--set id=value` sets a parameter by its id. Written as 32-bit float stereo at 48 kHz.

use audiostrada_dsp::{MidiEvent, create, info};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;
const PHRASE: [[u8; 4]; 4] = [
    [48, 55, 64, 71],
    [45, 52, 60, 67],
    [50, 57, 65, 72],
    [43, 53, 59, 65],
];

/// A note: start and length in seconds, key, velocity.
type Note = (f64, f64, u8, u8);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage =
        "render_keys <id> <out.wav> notes|chord <velocity> <keys>|phrase [--set id=value]...";
    let id = args.first().expect(usage).clone();
    let out = args.get(1).expect(usage).clone();
    let mut rest = args[2..].iter();
    let mut notes: Vec<Note> = Vec::new();
    let mut sets = Vec::new();
    let mut length = 0.0;
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "notes" => {
                let mut at = 0.0;
                for key in (28..=98).step_by(7) {
                    for velocity in [20, 45, 70, 95, 120] {
                        notes.push((at, 4.0, key, velocity));
                        at += 6.0;
                    }
                }
                length = at;
            }
            "chord" => {
                let velocity: u8 = rest.next().expect(usage).parse().expect("velocity");
                for key in rest.next().expect(usage).split(',') {
                    notes.push((0.0, 2.0, key.parse().expect("key"), velocity));
                }
                length = 4.0;
            }
            "phrase" => {
                for (k, chord) in PHRASE.iter().enumerate() {
                    for &key in chord {
                        notes.push((k as f64 * 2.0, 1.9, key, 90));
                    }
                }
                length = 11.0;
            }
            "--set" => sets.push(rest.next().expect(usage).clone()),
            other => panic!("unknown argument {other}\n{usage}"),
        }
    }
    let params = info(&id)
        .unwrap_or_else(|| panic!("no instrument {id}"))
        .params;
    let mut effect = create(&id).expect("instrument");
    effect.prepare(RATE, BLOCK);
    for set in &sets {
        let (name, value) = set.split_once('=').expect("id=value");
        let i = params
            .iter()
            .position(|p| p.id == name)
            .unwrap_or_else(|| panic!("{id} has no parameter {name}"));
        effect.set_param(i, value.parse().expect("value"));
    }
    // Every note's on and off, by sample.
    let mut events: Vec<(u64, MidiEvent)> = Vec::new();
    for &(start, hold, key, velocity) in &notes {
        let at = |s: f64| (s * f64::from(RATE)) as u64;
        events.push((at(start), MidiEvent::note_on(0, 0, key, velocity)));
        events.push((at(start + hold), MidiEvent::note_off(0, 0, key)));
    }
    events.sort_by_key(|e| e.0);
    let total = (length * f64::from(RATE)) as usize;
    let mut audio = Vec::with_capacity(total * 2);
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut next = 0;
    let mut done = 0;
    while done < total {
        let n = BLOCK.min(total - done);
        // Events land at the start of the block they fall in.
        while next < events.len() && events[next].0 < (done + n) as u64 {
            effect.midi(&events[next].1);
            next += 1;
        }
        effect.process(&mut l[..n], &mut r[..n]);
        for j in 0..n {
            audio.push(l[j]);
            audio.push(r[j]);
        }
        done += n;
    }
    write_wav(&out, &audio);
    let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    println!(
        "{out}: {:.1} s, peak {:.1} dBFS",
        length,
        20.0 * peak.max(1e-10).log10()
    );
}

fn write_wav(path: &str, samples: &[f32]) {
    let data = (samples.len() * 4) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 8).to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).unwrap_or_else(|e| panic!("{path}: {e}"));
}
