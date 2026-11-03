//! Music theory for the piano roll: scales, chords (building and naming) and the chord track.

use audiostrada_core::Note;
pub use audiostrada_dsp::pitch_correct::{NOTE_NAMES, SCALES};

/// A scale from a root (0 = C), one of [`SCALES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale {
    pub root: u8,
    pub mode: usize,
}

impl Scale {
    fn degrees(self) -> &'static [bool; 12] {
        &SCALES[self.mode.min(SCALES.len() - 1)].1
    }

    pub fn contains(self, key: u8) -> bool {
        self.degrees()[usize::from((key % 12 + 12 - self.root % 12) % 12)]
    }

    pub fn name(self) -> String {
        format!(
            "{} {}",
            NOTE_NAMES[usize::from(self.root % 12)],
            crate::i18n::tr(SCALES[self.mode.min(SCALES.len() - 1)].0)
        )
    }

    /// The scale key nearest to `key` (below first on a tie).
    pub fn snap(self, key: u8) -> u8 {
        (0..12u8)
            .flat_map(|d| [key.saturating_sub(d), key.saturating_add(d).min(127)])
            .find(|&k| self.contains(k))
            .unwrap_or(key)
    }

    /// `key` moved by `steps` notes of the scale (from its nearest scale key).
    pub fn step(self, key: u8, steps: i32) -> u8 {
        let mut k = i32::from(self.snap(key));
        let dir = steps.signum();
        for _ in 0..steps.abs() {
            loop {
                k += dir;
                if !(0..=127).contains(&k) {
                    return self.snap(k.clamp(0, 127) as u8);
                }
                if self.contains(k as u8) {
                    break;
                }
            }
        }
        k as u8
    }
}

/// Chord qualities, by their symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordKind {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Power,
    Sixth,
    Minor6,
    Seventh,
    Major7,
    Minor7,
    HalfDiminished,
    Diminished7,
    Add9,
    Ninth,
    Major9,
    Minor9,
}

impl ChordKind {
    pub const ALL: [ChordKind; 18] = [
        ChordKind::Major,
        ChordKind::Minor,
        ChordKind::Diminished,
        ChordKind::Augmented,
        ChordKind::Sus2,
        ChordKind::Sus4,
        ChordKind::Power,
        ChordKind::Sixth,
        ChordKind::Minor6,
        ChordKind::Seventh,
        ChordKind::Major7,
        ChordKind::Minor7,
        ChordKind::HalfDiminished,
        ChordKind::Diminished7,
        ChordKind::Add9,
        ChordKind::Ninth,
        ChordKind::Major9,
        ChordKind::Minor9,
    ];

    /// The symbol after the root ("" for major).
    pub fn symbol(self) -> &'static str {
        match self {
            ChordKind::Major => "",
            ChordKind::Minor => "m",
            ChordKind::Diminished => "dim",
            ChordKind::Augmented => "aug",
            ChordKind::Sus2 => "sus2",
            ChordKind::Sus4 => "sus4",
            ChordKind::Power => "5",
            ChordKind::Sixth => "6",
            ChordKind::Minor6 => "m6",
            ChordKind::Seventh => "7",
            ChordKind::Major7 => "maj7",
            ChordKind::Minor7 => "m7",
            ChordKind::HalfDiminished => "m7b5",
            ChordKind::Diminished7 => "dim7",
            ChordKind::Add9 => "add9",
            ChordKind::Ninth => "9",
            ChordKind::Major9 => "maj9",
            ChordKind::Minor9 => "m9",
        }
    }

    pub fn intervals(self) -> &'static [u8] {
        match self {
            ChordKind::Major => &[0, 4, 7],
            ChordKind::Minor => &[0, 3, 7],
            ChordKind::Diminished => &[0, 3, 6],
            ChordKind::Augmented => &[0, 4, 8],
            ChordKind::Sus2 => &[0, 2, 7],
            ChordKind::Sus4 => &[0, 5, 7],
            ChordKind::Power => &[0, 7, 12],
            ChordKind::Sixth => &[0, 4, 7, 9],
            ChordKind::Minor6 => &[0, 3, 7, 9],
            ChordKind::Seventh => &[0, 4, 7, 10],
            ChordKind::Major7 => &[0, 4, 7, 11],
            ChordKind::Minor7 => &[0, 3, 7, 10],
            ChordKind::HalfDiminished => &[0, 3, 6, 10],
            ChordKind::Diminished7 => &[0, 3, 6, 9],
            ChordKind::Add9 => &[0, 4, 7, 14],
            ChordKind::Ninth => &[0, 4, 7, 10, 14],
            ChordKind::Major9 => &[0, 4, 7, 11, 14],
            ChordKind::Minor9 => &[0, 3, 7, 10, 14],
        }
    }
}

/// How the notes of a chord are spread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voicing {
    Close,
    /// The second note from the bottom an octave up.
    Open,
    /// The second note from the top an octave down.
    Drop2,
    /// Root low, the others an octave up.
    Spread,
}

impl Voicing {
    pub const ALL: [Voicing; 4] = [
        Voicing::Close,
        Voicing::Open,
        Voicing::Drop2,
        Voicing::Spread,
    ];

    pub fn name(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Voicing::Close => tr("Close"),
            Voicing::Open => tr("Open"),
            Voicing::Drop2 => tr("Drop 2"),
            Voicing::Spread => tr("Spread"),
        }
    }
}

/// The keys of a chord on `root`: intervals from it, inverted `inversion` times (the lowest
/// notes go up an octave), then voiced.
pub fn build_chord(root: u8, intervals: &[u8], inversion: usize, voicing: Voicing) -> Vec<u8> {
    let mut keys: Vec<i32> = intervals
        .iter()
        .map(|&i| i32::from(root) + i32::from(i))
        .collect();
    keys.sort_unstable();
    for _ in 0..inversion % keys.len().max(1) {
        let low = keys.remove(0);
        keys.push(low + 12);
    }
    keys.sort_unstable();
    match voicing {
        Voicing::Close => {}
        Voicing::Open if keys.len() >= 3 => keys[1] += 12,
        Voicing::Drop2 if keys.len() >= 3 => {
            let i = keys.len() - 2;
            keys[i] -= 12;
        }
        Voicing::Spread if keys.len() >= 2 => {
            for k in keys.iter_mut().skip(1) {
                *k += 12;
            }
        }
        _ => {}
    }
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter(|k| (0..=127).contains(k))
        .map(|k| k as u8)
        .collect()
}

/// The chord the scale builds on `key`: thirds stacked in the scale (three or four notes).
pub fn diatonic_intervals(scale: Scale, key: u8, notes: usize) -> Vec<u8> {
    let root = scale.snap(key);
    (0..notes as i32)
        .map(|i| scale.step(root, 2 * i).saturating_sub(root))
        .collect()
}

/// The name of the chord formed by `keys` ("Am7", "C/E"), if they make one.
pub fn chord_name(keys: &[u8]) -> Option<String> {
    let mut classes: Vec<u8> = keys.iter().map(|k| k % 12).collect();
    classes.sort_unstable();
    classes.dedup();
    if classes.len() < 2 {
        return None;
    }
    let bass = keys.iter().min()? % 12;
    // Richer chords first, so a seventh is not named by the triad inside it; the bass as root
    // before any other note.
    let mut kinds = ChordKind::ALL.to_vec();
    kinds.sort_by_key(|k| std::cmp::Reverse(k.intervals().len()));
    let roots = std::iter::once(bass).chain(classes.iter().copied().filter(|&c| c != bass));
    for root in roots {
        let mut moved: Vec<u8> = classes.iter().map(|c| (c + 12 - root) % 12).collect();
        moved.sort_unstable();
        for &kind in &kinds {
            let mut shape: Vec<u8> = kind.intervals().iter().map(|i| i % 12).collect();
            shape.sort_unstable();
            shape.dedup();
            if moved == shape {
                let mut name = format!("{}{}", NOTE_NAMES[usize::from(root)], kind.symbol());
                if bass != root {
                    name.push('/');
                    name.push_str(NOTE_NAMES[usize::from(bass)]);
                }
                return Some(name);
            }
        }
    }
    None
}

/// The chords the notes form over time: start, end (clip samples) and name, consecutive
/// identical ones merged, those shorter than `shortest` left out. Notes shorter than that
/// (hits, passing notes) do not count.
pub fn chord_track(notes: &[Note], shortest: u64) -> Vec<(u64, u64, String)> {
    // A sweep over the starts and ends, with the number of notes sounding on each key.
    let mut events: Vec<(u64, i8, u8)> = notes
        .iter()
        .filter(|n| !n.muted && n.length >= shortest)
        .flat_map(|n| {
            [
                (n.start, 1, n.key & 127),
                (n.start + n.length, -1, n.key & 127),
            ]
        })
        .collect();
    events.sort_unstable_by_key(|e| (e.0, e.1));
    let mut count = [0i32; 128];
    let mut out: Vec<(u64, u64, String)> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let a = events[i].0;
        while i < events.len() && events[i].0 == a {
            count[usize::from(events[i].2)] += i32::from(events[i].1);
            i += 1;
        }
        let Some(&(b, _, _)) = events.get(i) else {
            break;
        };
        let sounding: Vec<u8> = (0..128u8).filter(|&k| count[usize::from(k)] > 0).collect();
        let Some(name) = (sounding.len() >= 3)
            .then(|| chord_name(&sounding))
            .flatten()
        else {
            continue;
        };
        match out.last_mut() {
            Some(last) if last.2 == name && last.1 == a => last.1 = b,
            _ => out.push((a, b, name)),
        }
    }
    out.retain(|(a, b, _)| b - a >= shortest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const C_MAJOR: Scale = Scale { root: 0, mode: 0 };

    #[test]
    fn scales_snap_and_step() {
        assert!(C_MAJOR.contains(64) && !C_MAJOR.contains(61));
        assert_eq!(C_MAJOR.snap(61), 60);
        assert_eq!(C_MAJOR.step(60, 2), 64);
        assert_eq!(C_MAJOR.step(64, -3), 59);
        let a_minor = Scale { root: 9, mode: 1 };
        assert!(a_minor.contains(72) && !a_minor.contains(68));
    }

    #[test]
    fn chords_are_built_inverted_and_voiced() {
        let triad = ChordKind::Major.intervals();
        assert_eq!(build_chord(60, triad, 0, Voicing::Close), [60, 64, 67]);
        assert_eq!(build_chord(60, triad, 1, Voicing::Close), [64, 67, 72]);
        assert_eq!(build_chord(60, triad, 0, Voicing::Open), [60, 67, 76]);
        let seventh = ChordKind::Major7.intervals();
        assert_eq!(
            build_chord(60, seventh, 0, Voicing::Drop2),
            [55, 60, 64, 71]
        );
        assert_eq!(diatonic_intervals(C_MAJOR, 62, 3), [0, 3, 7], "D minor");
        assert_eq!(diatonic_intervals(C_MAJOR, 67, 4), [0, 4, 7, 10], "G7");
    }

    #[test]
    fn chords_are_named_with_their_bass() {
        assert_eq!(chord_name(&[60, 64, 67]).as_deref(), Some("C"));
        assert_eq!(chord_name(&[57, 60, 64, 67]).as_deref(), Some("Am7"));
        assert_eq!(chord_name(&[52, 60, 67]).as_deref(), Some("C/E"));
        assert_eq!(chord_name(&[59, 62, 65]).as_deref(), Some("Bdim"));
        assert_eq!(chord_name(&[60, 61, 62]), None);
    }

    #[test]
    fn the_chord_track_follows_the_harmony() {
        let note = |start, length, key| Note {
            start,
            length,
            key,
            ..Default::default()
        };
        let notes = [
            note(0, 100, 60),
            note(0, 100, 64),
            note(0, 100, 67),
            note(10, 20, 72),
            note(20, 5, 74),
            note(100, 100, 57),
            note(100, 100, 60),
            note(100, 100, 64),
        ];
        let track = chord_track(&notes, 10);
        let names: Vec<&str> = track.iter().map(|t| t.2.as_str()).collect();
        assert_eq!(names, ["C", "Am"]);
        assert_eq!(
            (track[0].0, track[0].1),
            (0, 100),
            "merged across the passing note"
        );
    }
}
