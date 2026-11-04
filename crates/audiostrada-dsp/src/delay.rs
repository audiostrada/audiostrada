//! Stereo delay with feedback, filters in the feedback path, ping-pong and dry/wet mix. The delay
//! time glides when changed on the Tape and Analog models, which add wow and flutter,
//! saturation in the loop and darker repeats; the Digital model crossfades to the new time
//! instead, without a pitch sweep. The right side can be offset, the echoes duck while the
//! input plays, and freeze holds the loop as it is.

use crate::line::Line;
use crate::{
    Effect, EffectInfo, ParamInfo, choice, lowpass_coefficient, param, pole, switch,
    time_coefficient,
};

/// Two bars at 60 BPM.
const MAX_SECONDS: f32 = 8.0;
const CROSSFADE_MS: f32 = 40.0;
/// Input level from which ducking is at its full depth.
const DUCK_FULL: f32 = 0.25;

pub const INFO: EffectInfo = EffectInfo {
    id: "delay",
    name: "Strada-Delay",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Space",
    description: "Digital, tape or analog echoes, ping-pong and tempo sync.",
};

/// Note values for the synced time, and their length in quarter notes; added values come
/// last so that saved projects keep their note.
const NOTES: [(&str, f32); 15] = [
    ("1/32", 0.125),
    ("1/16 T", 1.0 / 6.0),
    ("1/16", 0.25),
    ("1/16 D", 0.375),
    ("1/8 T", 1.0 / 3.0),
    ("1/8", 0.5),
    ("1/8 D", 0.75),
    ("1/4 T", 2.0 / 3.0),
    ("1/4", 1.0),
    ("1/4 D", 1.5),
    ("1/2", 2.0),
    ("1 bar", 4.0),
    ("1/2 T", 4.0 / 3.0),
    ("1/2 D", 3.0),
    ("2 bars", 8.0),
];

pub const NOTE_NAMES: [&str; 15] = [
    NOTES[0].0,
    NOTES[1].0,
    NOTES[2].0,
    NOTES[3].0,
    NOTES[4].0,
    NOTES[5].0,
    NOTES[6].0,
    NOTES[7].0,
    NOTES[8].0,
    NOTES[9].0,
    NOTES[10].0,
    NOTES[11].0,
    NOTES[12].0,
    NOTES[13].0,
    NOTES[14].0,
];

/// The note values from the shortest, for the editor's grid.
pub fn notes_by_length() -> Vec<(usize, &'static str, f32)> {
    let mut notes: Vec<_> = NOTES
        .iter()
        .enumerate()
        .map(|(i, (name, quarters))| (i, *name, *quarters))
        .collect();
    notes.sort_by(|a, b| a.2.total_cmp(&b.2));
    notes
}

pub const MODELS: [&str; 3] = ["Digital", "Tape", "Analog"];
pub const MODEL_HINTS: [&str; 3] = [
    "Clean repeats; a new time crossfades in without a pitch sweep",
    "Wobbling, saturated repeats that darken; a new time glides like a varispeed",
    "Bucket-brigade: darker the longer the time, drifting and warm",
];

const PARAMS: [ParamInfo; COUNT] = [
    param("time", "Time", 1.0, 4000.0, 375.0, "ms"),
    param("feedback", "Feedback", 0.0, 95.0, 35.0, "%"),
    switch("pingpong", "Ping-pong", 0.0),
    param("low_cut", "Low cut", 20.0, 2000.0, 100.0, "Hz"),
    param("high_cut", "High cut", 1000.0, 20000.0, 8000.0, "Hz"),
    param("mix", "Mix", 0.0, 100.0, 30.0, "%"),
    switch("sync", "Sync to tempo", 0.0),
    choice("note", "Note", &NOTE_NAMES, 6.0),
    choice("model", "Model", &MODELS, 0.0),
    param("wow", "Wow & flutter", 0.0, 100.0, 25.0, "%"),
    param("drive", "Drive", 0.0, 100.0, 30.0, "%"),
    param("offset", "Right offset", -50.0, 50.0, 0.0, "ms"),
    param("duck", "Ducking", 0.0, 100.0, 0.0, "%"),
    switch("freeze", "Freeze", 0.0),
];

const OFFSET: usize = 11;
const DUCK: usize = 12;
const FREEZE: usize = 13;
const COUNT: usize = 14;

#[derive(Debug)]
pub struct Delay {
    rate: f32,
    values: [f32; COUNT],
    bpm: f64,
    /// Digital model: the time read, the time faded to, and how far the fade is (0 to 1).
    heads: (f32, f32, f32),
    envelope: f32,
    phase: [f32; 2],
    dark: [f32; 2],
    lines: [Line; 2],
    /// Current delay in samples (glides towards the target).
    current: f32,
    glide: f32,
    lp: [f32; 2],
    hp: [f32; 2],
}

impl Default for Delay {
    fn default() -> Self {
        let mut delay = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            bpm: 120.0,
            heads: (0.0, 0.0, 1.0),
            envelope: 0.0,
            phase: [0.0, 1.3],
            dark: [0.0; 2],
            lines: Default::default(),
            current: 0.0,
            glide: 0.0,
            lp: [0.0; 2],
            hp: [0.0; 2],
        };
        delay.allocate();
        delay
    }
}

/// Length of a note value (index into the "note" choices) at `bpm`, in seconds.
pub fn synced_seconds(note: f32, bpm: f64) -> f32 {
    let quarters = NOTES[(note.max(0.0).round() as usize).min(NOTES.len() - 1)].1;
    quarters * 60.0 / bpm.max(1.0) as f32
}

impl Delay {
    /// Delay time in seconds, the note's length when synced to the tempo.
    fn seconds(&self) -> f32 {
        if self.values[6] >= 0.5 {
            synced_seconds(self.values[7], self.bpm)
        } else {
            self.values[0] * 0.001
        }
    }

    fn max_delay(&self) -> f32 {
        (self.lines[0].len() - 2) as f32
    }

    /// Delay in samples for `seconds`, within the line.
    fn target(&self, seconds: f32) -> f32 {
        (seconds * self.rate).clamp(1.0, self.max_delay())
    }

    fn allocate(&mut self) {
        let frames = (MAX_SECONDS * self.rate) as usize + 2;
        self.lines = [Line::new(frames), Line::new(frames)];
        self.current = self.target(self.seconds());
        self.heads = (self.current, self.current, 1.0);
        self.envelope = 0.0;
        self.glide = time_coefficient(50.0, self.rate);
        self.lp = [0.0; 2];
        self.hp = [0.0; 2];
        self.dark = [0.0; 2];
    }
}

impl Effect for Delay {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [
            _,
            feedback,
            pingpong,
            low_cut,
            high_cut,
            mix,
            _,
            _,
            model,
            wow,
            drive,
            ..,
        ] = self.values;
        let model = model.round().clamp(0.0, 2.0) as usize;
        let max = self.max_delay();
        let seconds = self.seconds();
        let target = self.target(seconds);
        let feedback = feedback / 100.0;
        let pingpong = pingpong >= 0.5;
        let lp = pole(high_cut, self.rate);
        let hp = pole(low_cut, self.rate);
        let (wet, dry) = (mix / 100.0, 1.0 - mix / 100.0);
        // Tape wobbles (wow and flutter), analog drifts; both darken and saturate repeats.
        let (depth, wow_hz, flutter) = match model {
            1 => (wow / 100.0 * 0.004, 0.55, 0.25),
            2 => (wow / 100.0 * 0.002, 0.3, 0.0),
            _ => (0.0, 0.0, 0.0),
        };
        let steps = [
            std::f32::consts::TAU * wow_hz / self.rate,
            std::f32::consts::TAU * 6.1 / self.rate,
        ];
        let saturation = if model == 0 {
            0.0
        } else {
            0.5 + drive / 100.0 * 3.0
        };
        let dark_hz = match model {
            1 => 6000.0,
            2 => (2500.0 * (0.4 / seconds.max(0.02)).sqrt()).clamp(1200.0, 7000.0),
            _ => self.rate,
        };
        let dark = lowpass_coefficient(dark_hz, self.rate);
        let offset = self.values[OFFSET] * 0.001 * self.rate;
        let duck = self.values[DUCK] / 100.0;
        let frozen = self.values[FREEZE] >= 0.5;
        let fade_step = 1.0 / (CROSSFADE_MS * 0.001 * self.rate);
        let (duck_attack, duck_release) = (
            1.0 - time_coefficient(5.0, self.rate),
            1.0 - time_coefficient(250.0, self.rate),
        );
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            // Where to read: a gliding time on tape and analog, two crossfaded heads in digital.
            let (times, blend) = if model == 0 {
                let (from, to, fade) = &mut self.heads;
                if *fade >= 1.0 && (target - *from).abs() > 0.5 {
                    (*to, *fade) = (target, 0.0);
                }
                if *fade < 1.0 {
                    *fade = (*fade + fade_step).min(1.0);
                    if *fade >= 1.0 {
                        *from = *to;
                    }
                }
                ([*from, *to], *fade)
            } else {
                self.current = target + self.glide * (self.current - target);
                let mut time = self.current;
                if depth > 0.0 && !frozen {
                    self.phase[0] = (self.phase[0] + steps[0]) % std::f32::consts::TAU;
                    self.phase[1] = (self.phase[1] + steps[1]) % std::f32::consts::TAU;
                    let wobble = self.phase[0].sin() + flutter * self.phase[1].sin();
                    time = (time * (1.0 + depth * wobble)).clamp(1.0, max);
                }
                ([time, time], 1.0)
            };
            let read = |line: &Line, extra: f32| {
                let at = |t: f32| line.read((t + extra).clamp(1.0, max));
                let new = at(times[1]);
                if blend >= 1.0 {
                    new
                } else {
                    // Equal power, so the level holds through the fade.
                    let a = blend * std::f32::consts::FRAC_PI_2;
                    at(times[0]) * a.cos() + new * a.sin()
                }
            };
            let raw = [read(&self.lines[0], 0.0), read(&self.lines[1], offset)];
            let mut echoes = raw;
            if model != 0 {
                for (ch, e) in echoes.iter_mut().enumerate() {
                    self.dark[ch] += dark * (*e - self.dark[ch]);
                    *e = (self.dark[ch] * saturation).tanh() / saturation;
                }
            }
            for (ch, e) in echoes.iter_mut().enumerate() {
                self.lp[ch] = *e + lp * (self.lp[ch] - *e);
                self.hp[ch] = self.lp[ch] + hp * (self.hp[ch] - self.lp[ch]);
                *e = self.lp[ch] - self.hp[ch];
            }
            let (input_l, input_r) = (*l, *r);
            // Frozen: the loop goes round as it is, without the input nor any loss.
            let (fb_l, fb_r) = if frozen {
                (raw[0], raw[1])
            } else if pingpong {
                (
                    (input_l + input_r) * 0.5 + echoes[1] * feedback,
                    echoes[0] * feedback,
                )
            } else {
                (
                    input_l + echoes[0] * feedback,
                    input_r + echoes[1] * feedback,
                )
            };
            self.lines[0].push(fb_l);
            self.lines[1].push(fb_r);
            let out = if frozen { raw } else { echoes };
            let level = input_l.abs().max(input_r.abs());
            let k = if level > self.envelope {
                duck_attack
            } else {
                duck_release
            };
            self.envelope += k * (level - self.envelope);
            let gain = wet * (1.0 - duck * (self.envelope / DUCK_FULL).min(1.0));
            *l = input_l * dry + out[0] * gain;
            *r = input_r * dry + out[1] * gain;
        }
    }

    // Runs on the audio thread after jumps: clears in place, never reallocates.
    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.current = self.target(self.seconds());
        self.heads = (self.current, self.current, 1.0);
        self.envelope = 0.0;
        self.lp = [0.0; 2];
        self.hp = [0.0; 2];
        self.dark = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_arrives_after_the_delay_time() {
        let mut delay = Delay::default();
        delay.set_param(0, 100.0);
        delay.set_param(3, 20.0);
        delay.set_param(4, 20000.0);
        delay.set_param(5, 100.0);
        delay.prepare(48_000, 4800);
        let mut l = vec![0.0f32; 9600];
        l[0] = 1.0;
        let mut r = l.clone();
        delay.process(&mut l, &mut r);
        let peak = l
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0;
        assert!((peak as i64 - 4800).abs() <= 2, "echo at {peak}");
        assert!(l.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn synced_time_follows_the_tempo() {
        for (bpm, note, expected) in [(120.0, 8.0, 24_000), (100.0, 5.0, 14_400)] {
            let mut delay = Delay::default();
            delay.set_param(3, 20.0);
            delay.set_param(4, 20000.0);
            delay.set_param(5, 100.0);
            delay.set_param(6, 1.0);
            delay.set_param(7, note);
            delay.set_tempo(bpm);
            delay.prepare(48_000, 48_000);
            // Settle the glide on the synced time before the impulse.
            let (mut l, mut r) = (vec![0.0f32; 48_000], vec![0.0f32; 48_000]);
            delay.process(&mut l, &mut r);
            let (mut l, mut r) = (vec![0.0f32; 48_000], vec![0.0f32; 48_000]);
            l[0] = 1.0;
            r[0] = 1.0;
            delay.process(&mut l, &mut r);
            let peak = (1..l.len())
                .max_by(|&a, &b| l[a].abs().total_cmp(&l[b].abs()))
                .unwrap();
            assert!(
                (peak as i64 - expected).abs() <= 2,
                "{bpm} BPM: echo at {peak}"
            );
        }
    }

    #[test]
    fn a_reset_starts_on_the_synced_time_without_gliding() {
        let mut delay = Delay::default();
        delay.set_param(6, 1.0);
        delay.set_param(7, 8.0);
        delay.prepare(48_000, 256);
        assert_eq!(delay.current, 24_000.0, "a quarter note at 120 BPM");
        delay.set_tempo(100.0);
        delay.reset();
        assert!((delay.current - 28_800.0).abs() < 0.01);
        delay.set_param(6, 0.0);
        delay.set_param(0, 20_000.0);
        delay.reset();
        assert_eq!(delay.current, delay.max_delay(), "clamped to the line");
    }

    #[test]
    fn ping_pong_bounces_the_echoes_between_the_sides() {
        let mut delay = Delay::default();
        delay.set_param(0, 10.0);
        delay.set_param(1, 50.0);
        delay.set_param(2, 1.0);
        delay.set_param(3, 20.0);
        delay.set_param(4, 20000.0);
        delay.set_param(5, 100.0);
        delay.prepare(48_000, 4800);
        let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
        l[0] = 1.0;
        delay.process(&mut l, &mut r);
        let peak = |x: &[f32]| (1..x.len()).max_by(|&a, &b| x[a].abs().total_cmp(&x[b].abs()));
        assert!(
            (peak(&l).unwrap() as i64 - 480).abs() <= 2,
            "first echo left"
        );
        assert!(
            (peak(&r).unwrap() as i64 - 960).abs() <= 2,
            "second echo right"
        );
    }

    #[test]
    fn tape_and_analog_repeats_wobble_darken_and_stay_bounded() {
        for model in [1.0, 2.0] {
            let mut delay = Delay::default();
            delay.set_param(0, 100.0);
            delay.set_param(1, 95.0);
            delay.set_param(4, 20000.0);
            delay.set_param(5, 100.0);
            delay.set_param(8, model);
            delay.set_param(9, 100.0);
            delay.set_param(10, 100.0);
            delay.prepare(48_000, 4800);
            let mut out = Vec::new();
            for block in 0..40 {
                let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..4800)
                    .map(|i| {
                        let s = if block == 0 && i < 480 {
                            (i as f32 * 0.9).sin()
                        } else {
                            0.0
                        };
                        (s, s)
                    })
                    .unzip();
                delay.process(&mut l, &mut r);
                out.extend(l);
            }
            assert!(
                out.iter().all(|s| s.is_finite() && s.abs() < 1.5),
                "model {model}"
            );
            let early: f32 = out[4800..9600].iter().map(|s| s * s).sum();
            let late: f32 = out[150_000..].iter().map(|s| s * s).sum();
            assert!(
                early > 0.01 && late < early,
                "model {model}: {early} {late}"
            );
        }
    }

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    fn clean(changes: &[(&str, f32)]) -> Delay {
        let mut delay = Delay::default();
        for (id, v) in [("low_cut", 20.0), ("high_cut", 20000.0), ("mix", 100.0)]
            .into_iter()
            .chain(changes.iter().copied())
        {
            delay.set_param(index(id), v);
        }
        delay.prepare(48_000, 4800);
        delay
    }

    fn first_peak(x: &[f32]) -> usize {
        (1..x.len())
            .max_by(|&a, &b| x[a].abs().total_cmp(&x[b].abs()))
            .unwrap()
    }

    #[test]
    fn long_synced_notes_are_not_cut_short() {
        // A bar at 40 BPM: 6 s, three times what the delay held before.
        let mut delay = clean(&[("sync", 1.0), ("note", 11.0)]);
        delay.set_tempo(40.0);
        delay.reset();
        assert_eq!(delay.current, 288_000.0);
        assert_eq!(synced_seconds(14.0, 60.0), 8.0, "two bars at 60 BPM");
    }

    #[test]
    fn the_right_side_can_be_offset() {
        let mut delay = clean(&[("time", 100.0), ("offset", 20.0), ("feedback", 0.0)]);
        let (mut l, mut r) = (vec![0.0f32; 9600], vec![0.0f32; 9600]);
        l[0] = 1.0;
        r[0] = 1.0;
        delay.process(&mut l, &mut r);
        assert!((first_peak(&l) as i64 - 4800).abs() <= 2);
        assert!((first_peak(&r) as i64 - 5760).abs() <= 2);
    }

    #[test]
    fn digital_crossfades_to_a_new_time() {
        let mut delay = clean(&[("time", 100.0)]);
        let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
        delay.process(&mut l, &mut r);
        delay.set_param(index("time"), 300.0);
        let (mut l, mut r) = (vec![0.0f32; 48], vec![0.0f32; 48]);
        delay.process(&mut l, &mut r);
        assert!(
            (delay.heads.1 - 14_400.0).abs() < 0.01,
            "the new time is read at once"
        );
        assert!(
            (delay.heads.0 - 4800.0).abs() < 0.01,
            "the old one fades out"
        );
        let (mut l, mut r) = (vec![0.0f32; 2400], vec![0.0f32; 2400]);
        delay.process(&mut l, &mut r);
        let (from, to, fade) = delay.heads;
        assert!(fade == 1.0 && from == to, "after 40 ms");
    }

    #[test]
    fn freeze_holds_the_loop_and_ducking_lowers_the_echoes() {
        let mut delay = clean(&[("time", 50.0), ("feedback", 0.0)]);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..2400)
            .map(|i| {
                let s = 0.5 * (i as f32 * 0.05).sin();
                (s, s)
            })
            .unzip();
        delay.process(&mut l, &mut r);
        delay.set_param(index("freeze"), 1.0);
        let mut last = 0.0;
        for _ in 0..40 {
            let (mut l, mut r) = (vec![0.0f32; 2400], vec![0.0f32; 2400]);
            delay.process(&mut l, &mut r);
            last = l.iter().map(|s| s * s).sum::<f32>();
        }
        assert!(
            last > 10.0,
            "still ringing after 2 s without feedback: {last}"
        );

        let echo = |duck: f32| {
            let mut delay = clean(&[("time", 50.0), ("feedback", 50.0), ("duck", duck)]);
            let (mut l, mut r) = (vec![0.4f32; 9600], vec![0.4f32; 9600]);
            for (i, s) in l.iter_mut().chain(r.iter_mut()).enumerate() {
                *s *= (i as f32 * 0.07).sin();
            }
            delay.process(&mut l, &mut r);
            l.iter().map(|o| o * o).sum::<f32>()
        };
        assert!(
            echo(100.0) < echo(0.0) * 0.2,
            "echoes under the playing input"
        );
    }
}
