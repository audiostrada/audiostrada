//! Stereo delay with feedback, filters in the feedback path, ping-pong and dry/wet mix. The delay
//! time glides when changed, read with linear interpolation. Tape and Analog models add wow
//! and flutter, saturation in the loop and darker repeats.

use crate::line::Line;
use crate::{
    Effect, EffectInfo, ParamInfo, choice, lowpass_coefficient, param, pole, switch,
    time_coefficient,
};

const MAX_SECONDS: f32 = 2.0;

pub const INFO: EffectInfo = EffectInfo {
    id: "delay",
    name: "Delay",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Space",
    description: "Digital, tape or analog echoes, ping-pong and tempo sync.",
};

/// Note values for the synced time, and their length in quarter notes.
const NOTES: [(&str, f32); 12] = [
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
];

const NOTE_NAMES: [&str; 12] = [
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
];

const MODELS: [&str; 3] = ["Digital", "Tape", "Analog"];

const PARAMS: [ParamInfo; 11] = [
    param("time", "Time", 1.0, 2000.0, 375.0, "ms"),
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
];

#[derive(Debug)]
pub struct Delay {
    rate: f32,
    values: [f32; 11],
    bpm: f64,
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
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.current = target + self.glide * (self.current - target);
            let mut time = self.current;
            if depth > 0.0 {
                self.phase[0] = (self.phase[0] + steps[0]) % std::f32::consts::TAU;
                self.phase[1] = (self.phase[1] + steps[1]) % std::f32::consts::TAU;
                let wobble = self.phase[0].sin() + flutter * self.phase[1].sin();
                time = (time * (1.0 + depth * wobble)).clamp(1.0, max);
            }
            let mut echoes = [self.lines[0].read(time), self.lines[1].read(time)];
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
            let (fb_l, fb_r) = if pingpong {
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
            *l = input_l * dry + echoes[0] * wet;
            *r = input_r * dry + echoes[1] * wet;
        }
    }

    // Runs on the audio thread after jumps: clears in place, never reallocates.
    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.current = self.target(self.seconds());
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
        delay.set_param(0, 5000.0);
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
}
