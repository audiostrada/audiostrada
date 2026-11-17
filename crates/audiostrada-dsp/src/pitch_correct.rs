//! Pitch Correct: follows the pitch of a voice, picks the nearest note of the chosen scale and
//! moves the voice there with TD-PSOLA, grains of two periods laid one output period apart,
//! which keeps the formants. The retune speed glides the correction, humanize corrects the
//! note's center only, and a note already within the tolerance is left as sung. The scale can
//! be one of its own (notes picked on a keyboard). The staircase mode puts every note flat on
//! its target at once, for the stepped voice of the first automatic corrections.
//!
//! The engine is in [`crate::correct`].

use crate::correct::{Corrector, Settings};
use crate::{Effect, EffectInfo, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "pitch_correct",
    name: "Strada-Correct",
    params: &PARAMS,
    meters: &[
        "Input pitch",
        "Output pitch",
        "Target note",
        "Clarity",
        "Key heard",
        "Scale heard",
        "Key certainty",
    ],
    instrument: false,
    files: &[],
    category: "Utility",
    description: "Automatic pitch correction for voice, from natural to the robot effect.",
};

pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Scales: name and the semitones in it from the root.
pub const SCALES: [(&str, [bool; 12]); 7] = [
    (
        "Major",
        [
            true, false, true, false, true, true, false, true, false, true, false, true,
        ],
    ),
    (
        "Minor",
        [
            true, false, true, true, false, true, false, true, true, false, true, false,
        ],
    ),
    (
        "Harmonic minor",
        [
            true, false, true, true, false, true, false, true, true, false, false, true,
        ],
    ),
    (
        "Dorian",
        [
            true, false, true, true, false, true, false, true, false, true, true, false,
        ],
    ),
    (
        "Pentatonic major",
        [
            true, false, true, false, true, false, false, true, false, true, false, false,
        ],
    ),
    (
        "Pentatonic minor",
        [
            true, false, false, true, false, true, false, true, false, false, true, false,
        ],
    ),
    (
        "Blues",
        [
            true, false, false, true, false, true, true, true, false, false, true, false,
        ],
    ),
];

/// Chromatic, then [`SCALES`], then the notes picked by hand.
pub const SCALE_NAMES: [&str; 9] = [
    "Chromatic",
    "Major",
    "Minor",
    "Harmonic minor",
    "Dorian",
    "Pentatonic major",
    "Pentatonic minor",
    "Blues",
    "Custom",
];
pub const CUSTOM: usize = 8;

/// How the voice is corrected.
pub const MODES: [&str; 3] = ["Natural", "Staircase", "Harmony"];
pub const MODE_HINTS: [&str; 3] = [
    "Follows the settings below, from a light touch to a tight correction",
    "Every note flat on its target at once: the stepped, synthetic voice",
    "Corrected as in Natural, with a second voice singing along in the key",
];
pub const STAIRCASE: usize = 1;
pub const HARMONY: usize = 2;
/// The harmony voice's interval, and its notes of the scale from the voice.
pub const HARMONIES: [&str; 7] = [
    "Third above",
    "Fifth above",
    "Sixth above",
    "Octave above",
    "Third below",
    "Fourth below",
    "Octave below",
];
const HARMONY_STEPS: [i32; 7] = [2, 4, 5, 7, -2, -3, -7];
/// Index of the mode among the parameters.
const MODE: usize = 8;
/// Meters of the key heard: its root (-1 without one), its scale as an index of
/// [`SCALE_NAMES`], and how sure it is (0 to 1).
pub const KEY_METER: usize = 4;

const PARAMS: [ParamInfo; 11] = [
    choice("key", "Key", &NOTE_NAMES, 0.0),
    choice("scale", "Scale", &SCALE_NAMES, 0.0),
    param("speed", "Retune speed", 0.0, 400.0, 25.0, "ms"),
    param("humanize", "Humanize", 0.0, 100.0, 0.0, "%"),
    param("amount", "Amount", 0.0, 100.0, 100.0, "%"),
    param("transpose", "Transpose", -12.0, 12.0, 0.0, "st"),
    param("notes", "Custom notes", 0.0, 4095.0, 4095.0, ""),
    param("tolerance", "Tolerance", 0.0, 50.0, 0.0, "cents"),
    choice("mode", "Mode", &MODES, 0.0),
    choice("harmony", "Harmony", &HARMONIES, 0.0),
    param("harmony_level", "Harmony level", 0.0, 100.0, 60.0, "%"),
];

/// Whether `note` (MIDI) belongs to `scale` (0 = chromatic) on `key`; the custom scale is
/// `notes`, a bit per pitch class from C.
pub fn in_scale(note: i32, key: i32, scale: usize, notes: u32) -> bool {
    if scale == CUSTOM {
        // No note picked would leave nothing to correct to: then every note.
        return notes & 0xfff == 0 || notes >> note.rem_euclid(12) & 1 == 1;
    }
    match scale.checked_sub(1).and_then(|s| SCALES.get(s)) {
        Some((_, steps)) => steps[(note - key).rem_euclid(12) as usize],
        None => true,
    }
}

/// The corrector of the voice, and in the harmony mode the one making the second voice.
#[derive(Debug)]
pub struct PitchCorrect {
    values: [f32; 11],
    engine: Corrector,
    harmony: Corrector,
    /// The input given to the harmony, then its output.
    scratch: [Vec<f32>; 2],
    /// The harmony was heard last block.
    harmonizing: bool,
}

impl Default for PitchCorrect {
    fn default() -> Self {
        Self {
            values: PARAMS.map(|p| p.default),
            engine: Corrector::new(48_000.0),
            harmony: Corrector::new(48_000.0),
            scratch: [Vec::new(), Vec::new()],
            harmonizing: false,
        }
    }
}

impl PitchCorrect {
    fn mode(&self) -> usize {
        (self.values[MODE].round().max(0.0) as usize).min(MODES.len() - 1)
    }

    fn settings(&self) -> Settings {
        let [
            key,
            scale,
            speed,
            humanize,
            amount,
            transpose,
            notes,
            tolerance,
            ..,
        ] = self.values;
        Settings {
            key: key.round() as i32,
            scale: scale.round().max(0.0) as usize,
            notes: notes.round().max(0.0) as u32,
            speed,
            humanize,
            amount,
            transpose,
            tolerance,
            staircase: self.mode() == STAIRCASE,
            harmony: 0,
        }
    }

    /// The harmony voice's settings: the voice's, aimed at the interval chosen.
    fn harmony_settings(&self) -> Settings {
        let interval = (self.values[9].round().max(0.0) as usize).min(HARMONIES.len() - 1);
        Settings {
            harmony: HARMONY_STEPS[interval],
            ..self.settings()
        }
    }
}

impl Effect for PitchCorrect {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        let rate = sample_rate as f32;
        for engine in [&mut self.engine, &mut self.harmony] {
            if rate != engine.rate() {
                engine.allocate(rate);
            } else {
                engine.reset();
            }
        }
        self.scratch = [vec![0.0; max_frames], vec![0.0; max_frames]];
        self.harmonizing = false;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let harmonizing = self.mode() == HARMONY && n <= self.scratch[0].len();
        if harmonizing && !self.harmonizing {
            self.harmony.reset();
        }
        self.harmonizing = harmonizing;
        if harmonizing {
            let settings = self.harmony_settings();
            let [l, r] = &mut self.scratch;
            let (l, r) = (&mut l[..n], &mut r[..n]);
            l.copy_from_slice(left);
            r.copy_from_slice(right);
            self.harmony.process(l, r, &settings);
        }
        let settings = self.settings();
        self.engine.process(left, right, &settings);
        if harmonizing {
            // Two voices that differ add up in power: the pair keeps the level of one.
            let level = self.values[10].clamp(0.0, 100.0) / 100.0;
            let keep = 1.0 / (1.0 + level * level).sqrt();
            let [l, r] = &self.scratch;
            for i in 0..n {
                left[i] = (left[i] + level * l[i]) * keep;
                right[i] = (right[i] + level * r[i]) * keep;
            }
        }
    }

    fn reset(&mut self) {
        self.engine.reset();
        self.harmony.reset();
    }

    fn latency(&self) -> u32 {
        self.engine.latency() as u32
    }

    fn meters(&self, out: &mut [f32]) {
        self.engine.meters(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biquad::{Biquad, Coefficients, FilterKind};

    fn to_note(hz: f32) -> f32 {
        69.0 + 12.0 * (hz / 440.0).log2()
    }

    fn to_hz(note: f32) -> f32 {
        440.0 * 2f32.powf((note - 69.0) / 12.0)
    }

    const RATE: f32 = 48_000.0;

    /// A voice-like tone: a fundamental and decaying harmonics.
    fn voice(hz: impl Fn(f32) -> f32, seconds: f32) -> Vec<f32> {
        let mut phase = 0.0f32;
        (0..(seconds * RATE) as usize)
            .map(|i| {
                phase += hz(i as f32 / RATE) / RATE;
                let t = phase * std::f32::consts::TAU;
                0.3 * t.sin() + 0.15 * (2.0 * t).sin() + 0.08 * (3.0 * t).sin()
            })
            .collect()
    }

    fn run(effect: &mut PitchCorrect, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        for block in input.chunks(256) {
            let (mut l, mut r) = (block.to_vec(), block.to_vec());
            effect.process(&mut l, &mut r);
            assert_eq!(l, r);
            out.extend(l);
        }
        out
    }

    fn corrector(settings: &[(usize, f32)]) -> PitchCorrect {
        let mut p = PitchCorrect::default();
        p.prepare(RATE as u32, 256);
        for &(i, v) in settings {
            p.set_param(i, v);
        }
        p
    }

    fn pitch_of(x: &[f32]) -> f32 {
        let mut difference = vec![0.0; 2048];
        crate::tuner::detect(&x[x.len() - 4096..], RATE, &mut difference)
            .unwrap()
            .0
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn brings_a_sharp_voice_to_the_note() {
        for (hz, expected) in [(220.0 * 2f32.powf(0.3 / 12.0), 220.0), (98.0 * 0.985, 98.0)] {
            let mut p = corrector(&[(2, 0.0)]);
            let out = run(&mut p, &voice(|_| hz, 1.0));
            let found = pitch_of(&out);
            let cents = 1200.0 * (found / expected).log2();
            assert!(
                cents.abs() < 4.0,
                "{hz} Hz came out at {found} ({cents} cents)"
            );
        }
    }

    #[test]
    fn follows_the_scale_and_the_key() {
        // C# is not in C major: a little above it, D is nearer than C.
        let hz = to_hz(61.1);
        let mut p = corrector(&[(1, 1.0), (2, 0.0)]);
        let out = run(&mut p, &voice(|_| hz, 1.0));
        let cents = 1200.0 * (pitch_of(&out) / to_hz(62.0)).log2();
        assert!(cents.abs() < 4.0, "{cents}");
        let mut meters = [0.0; 4];
        p.meters(&mut meters);
        assert_eq!(meters[2], 62.0);
        // In D major, C# is a note of the scale.
        let mut p = corrector(&[(0, 2.0), (1, 1.0), (2, 0.0)]);
        run(&mut p, &voice(|_| hz, 0.5));
        p.meters(&mut meters);
        assert_eq!(meters[2], 61.0);
        assert!(in_scale(61, 2, 1, 0) && !in_scale(61, 0, 1, 0) && in_scale(61, 0, 0, 0));
        // C, E and G picked: C# is out, E in, whatever the key.
        let triad = 1 | 1 << 4 | 1 << 7;
        assert!(!in_scale(61, 0, CUSTOM, triad) && in_scale(64, 5, CUSTOM, triad));
        assert!(in_scale(61, 0, CUSTOM, 0), "nothing picked: every note");
    }

    #[test]
    fn transposes_and_keeps_the_level() {
        let mut p = corrector(&[(2, 0.0), (5, 3.0)]);
        let input = voice(|_| 220.0, 1.0);
        let out = run(&mut p, &input);
        let cents = 1200.0 * (pitch_of(&out) / to_hz(to_note(220.0) + 3.0)).log2();
        assert!(cents.abs() < 4.0, "{cents}");
        let level = 20.0 * (rms(&out[24_000..]) / rms(&input[24_000..])).log10();
        assert!(level.abs() < 2.0, "{level} dB");
    }

    #[test]
    fn without_correction_the_sound_is_only_delayed() {
        let mut noise = 1u32;
        let hiss: Vec<f32> = (0..24_000)
            .map(|_| {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (noise >> 8) as f32 / (1u32 << 24) as f32 * 0.4 - 0.2
            })
            .collect();
        for (input, settings) in [(voice(|_| 180.0, 1.0), [(4, 0.0)]), (hiss, [(2, 0.0)])] {
            let mut p = corrector(&settings);
            let out = run(&mut p, &input);
            let delay = p.latency() as usize;
            let error = (12_000..input.len())
                .map(|i| (out[i] - input[i - delay]).abs())
                .fold(0.0f32, f32::max);
            assert!(error < 0.02, "{error}");
        }
    }

    #[test]
    fn retune_speed_glides_and_humanize_keeps_vibrato() {
        let mut meters = [0.0; 4];
        let sharp = to_hz(57.3);
        let mut p = corrector(&[(2, 200.0)]);
        run(&mut p, &voice(|_| sharp, 0.15));
        p.meters(&mut meters);
        let early = meters[1] - 57.0;
        run(&mut p, &voice(|_| sharp, 1.5));
        p.meters(&mut meters);
        assert!(early > 0.05 && early < 0.28, "glides: {early}");
        assert!((meters[1] - 57.0).abs() < 0.02, "arrives: {meters:?}");

        let vibrato = |t: f32| to_hz(57.0 + 0.4 * (t * 5.5 * std::f32::consts::TAU).sin());
        for (humanize, kept) in [(0.0, false), (100.0, true)] {
            let mut p = corrector(&[(2, 0.0), (3, humanize)]);
            let input = voice(vibrato, 1.0);
            let (mut low, mut high) = (f32::MAX, f32::MIN);
            for (i, block) in input.chunks(256).enumerate() {
                let (mut l, mut r) = (block.to_vec(), block.to_vec());
                p.process(&mut l, &mut r);
                p.meters(&mut meters);
                if i > 100 {
                    low = low.min(meters[1]);
                    high = high.max(meters[1]);
                }
            }
            assert_eq!(high - low > 0.4, kept, "humanize {humanize}: {low}..{high}");
        }
    }

    #[test]
    fn silence_stays_silent_after_the_latency() {
        let mut p = corrector(&[]);
        let delay = p.latency();
        assert!(delay > 1500 && delay < 2000, "{delay}");
        let out = run(&mut p, &vec![0.0; 9600]);
        assert!(out.iter().all(|s| *s == 0.0));
        p.prepare(96_000, 256);
        assert!(p.latency() > 2 * delay - 16);
        let out = run(&mut p, &voice(|_| 440.0, 0.3));
        assert!(out.iter().all(|s| s.is_finite()));
    }

    /// The settings by their place in the table.
    const SPEED: usize = 2;
    const HARMONY_LEVEL: usize = 10;

    /// A voice as a microphone hears it: the breath loud in its upper harmonics, around
    /// 2 kHz, `breath` times a full-scale noise.
    fn breathy(hz: f32, seconds: f32, breath: f32) -> Vec<f32> {
        let mut noise = 777u32;
        let band = Coefficients::design(FilterKind::BandPass, 2000.0, 1.5, 0.0, f64::from(RATE));
        let mut filter = Biquad::default();
        voice(|_| hz, seconds)
            .into_iter()
            .map(|s| {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (noise >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
                s + breath * filter.process(&band, f64::from(white)) as f32
            })
            .collect()
    }

    /// The meters after each block of `input`, from the block `skip` on.
    fn follow(p: &mut PitchCorrect, input: &[f32], skip: usize) -> Vec<[f32; 7]> {
        let mut seen = Vec::new();
        for (i, block) in input.chunks(256).enumerate() {
            let (mut l, mut r) = (block.to_vec(), block.to_vec());
            p.process(&mut l, &mut r);
            let mut meters = [0.0; 7];
            p.meters(&mut meters);
            if i >= skip {
                seen.push(meters);
            }
        }
        seen
    }

    /// The pitch of each stretch of 4096 samples of `x`, as notes.
    fn pitches(x: &[f32]) -> Vec<f32> {
        let mut difference = vec![0.0; 2048];
        x.as_chunks::<4096>()
            .0
            .iter()
            .filter_map(|w| crate::tuner::detect(w, RATE, &mut difference))
            .map(|found| to_note(found.0))
            .collect()
    }

    #[test]
    fn reads_the_pitch_of_a_voice_within_a_cent() {
        for hz in [82.4f32, 146.8, 261.6, 440.0, 880.0] {
            // The phase kept in double precision: the tone is on its pitch to the cent.
            let input: Vec<f32> = (0..(0.6 * RATE) as usize)
                .map(|i| {
                    let t = (f64::from(hz) * i as f64 / f64::from(RATE)).fract() as f32;
                    let t = t * std::f32::consts::TAU;
                    0.3 * t.sin() + 0.15 * (2.0 * t).sin() + 0.08 * (3.0 * t).sin()
                })
                .collect();
            let mut p = corrector(&[]);
            for meters in follow(&mut p, &input, 40) {
                let cents = 100.0 * (meters[0] - to_note(hz));
                assert!(cents.abs() < 1.0, "{hz} Hz read {cents} cents off");
            }
        }
    }

    #[test]
    fn follows_a_voice_under_its_breath() {
        // A3 sung 30 cents sharp, the breath 6 dB under the voice.
        let input = breathy(to_hz(57.3), 2.0, 0.8);
        let mut p = corrector(&[(SPEED, 0.0)]);
        let seen = follow(&mut p, &input, 40);
        let held = seen
            .iter()
            .filter(|m| m[0] > 0.0 && (m[1] - 57.0).abs() < 0.05)
            .count() as f32
            / seen.len() as f32;
        assert!(held > 0.8, "the note is held {held} of the time");
    }

    #[test]
    fn the_staircase_flattens_a_vibrato() {
        // 80 cents of vibrato from end to end around an A3 sung 20 cents sharp.
        let vibrato = |t: f32| to_hz(57.2 + 0.4 * (t * 5.5 * std::f32::consts::TAU).sin());
        let input = voice(vibrato, 2.0);
        let swing = |settings: &[(usize, f32)]| {
            let out = run(&mut corrector(settings), &input);
            let heard = pitches(&out[24_000..]);
            let (low, high) = heard
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), n| (lo.min(*n), hi.max(*n)));
            (low, high)
        };
        let (low, high) = swing(&[(MODE, STAIRCASE as f32)]);
        assert!(low > 56.95 && high < 57.05, "the staircase: {low}..{high}");
    }

    #[test]
    fn the_staircase_reaches_a_new_note_within_30_ms() {
        // Legato from an A3 to a C4, each a little off.
        let sung = |t: f32| to_hz(if t < 0.5 { 57.25 } else { 59.8 });
        let mut p = corrector(&[(MODE, STAIRCASE as f32)]);
        let seen = follow(&mut p, &voice(sung, 1.0), 0);
        let block = 256.0 / RATE;
        let before = seen
            .iter()
            .rposition(|m| m[2] == 57.0)
            .expect("the first note is followed");
        let after = seen
            .iter()
            .position(|m| m[2] == 60.0 && (m[1] - 60.0).abs() < 0.1)
            .expect("the second note is reached");
        let seconds = (after as f32 - before as f32) * block;
        assert!(seconds < 0.03, "{seconds} s from one note to the next");
        // And every reading after it is on the note.
        for m in &seen[after..] {
            assert!((m[1] - 60.0).abs() < 0.02, "{m:?}");
        }
    }

    #[test]
    fn a_change_of_note_leaves_no_click() {
        // A melody of half steps sung legato, moved to the staircase: the output bends no
        // more sharply from a sample to the next than the voice itself does.
        let melody = |t: f32| to_hz(57.3 + ((t / 0.25).floor() % 3.0) * 1.1);
        let input = voice(melody, 2.0);
        let sharpest = |x: &[f32]| {
            x.windows(3)
                .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
                .fold(0.0, f32::max)
        };
        for mode in [0, STAIRCASE] {
            let mut p = corrector(&[(SPEED, 0.0), (MODE, mode as f32)]);
            let out = run(&mut p, &input);
            let (dry, wet) = (sharpest(&input[4800..]), sharpest(&out[4800..]));
            assert!(wet < 1.25 * dry, "mode {mode}: {wet} against {dry}");
            let level = 20.0 * (rms(&out[24_000..]) / rms(&input[24_000..])).log10();
            assert!(level.abs() < 0.5, "mode {mode}: {level} dB");
        }
    }

    #[test]
    fn the_key_of_what_is_sung_is_heard() {
        // "Frère Jacques" and so on in D major, a quarter of a second a note.
        let tune = [
            0, 2, 4, 0, 0, 2, 4, 0, 4, 5, 7, 7, 4, 5, 7, 7, 9, 7, 5, 4, 0, 0,
        ];
        let sung = move |t: f32| to_hz(62.0 + tune[(t / 0.25) as usize % tune.len()] as f32);
        let mut p = corrector(&[]);
        let mut meters = [0.0; 7];
        p.meters(&mut meters);
        assert_eq!(meters[KEY_METER], -1.0, "nothing heard yet");
        let seen = follow(&mut p, &voice(sung, 6.0), 0);
        let last = seen.last().unwrap();
        assert_eq!(
            (last[KEY_METER], last[KEY_METER + 1]),
            (2.0, 1.0),
            "D major"
        );
        assert!(last[KEY_METER + 2] > 0.6, "{last:?}");
        // The key outlives a stop of the transport.
        p.reset();
        p.meters(&mut meters);
        assert_eq!(meters[KEY_METER], 2.0);
    }

    #[test]
    fn the_harmony_sings_a_third_above_in_the_key() {
        // An A3 sung a little sharp, in C major: the third above in the key is a C4.
        let mut p = corrector(&[(1, 1.0), (MODE, HARMONY as f32), (HARMONY_LEVEL, 100.0)]);
        let input = voice(|_| to_hz(57.2), 1.0);
        let out = run(&mut p, &input);
        let mut meters = [0.0; 7];
        p.harmony.meters(&mut meters);
        assert!(
            (meters[1] - 60.0).abs() < 0.05,
            "the harmony sings {}",
            meters[1]
        );
        p.meters(&mut meters);
        assert!(
            (meters[1] - 57.0).abs() < 0.05,
            "the voice sings {}",
            meters[1]
        );
        let level = 20.0 * (rms(&out[24_000..]) / rms(&input[24_000..])).log10();
        assert!(level.abs() < 1.5, "{level} dB");
    }

    #[test]
    fn a_note_within_the_tolerance_is_left_as_sung() {
        // 20 cents sharp of A3: corrected without tolerance, left alone with 30 cents.
        let sung = 220.0 * 2f32.powf(20.0 / 1200.0);
        for (tolerance, corrected) in [(0.0, true), (30.0, false)] {
            let mut p = corrector(&[(2, 0.0), (7, tolerance)]);
            let out = pitch_of(&run(&mut p, &voice(|_| sung, 1.0)));
            let cents = 1200.0 * (out / 220.0).log2();
            assert_eq!(
                cents.abs() < 8.0,
                corrected,
                "tolerance {tolerance}: {cents} cents"
            );
        }
    }
}
