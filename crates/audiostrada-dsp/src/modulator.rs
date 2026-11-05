//! Strada-Mod: the level, the side or the pitch moved in time. Tremolo, auto-pan, a cutter
//! playing a pattern of steps (trance gate), vibrato and ring modulation; free or synced to
//! the tempo, six shapes, the edges smoothed so that squares and steps never click.

use crate::filter::lfo_value;
use crate::line::Line;
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "modulator",
    name: "Strada-Mod",
    params: &PARAMS,
    meters: &["Gain", "Step", "Phase"],
    instrument: false,
    files: &[],
    category: "Modulation",
    description: "Tremolo, auto-pan, rhythmic cutter, vibrato and ring modulation, synced to \
                  the tempo: from vintage amp wobble to wild stutters.",
};

pub const MODES: [&str; 5] = ["Tremolo", "Auto-pan", "Cutter", "Vibrato", "Ring mod"];
pub const MODE_HINTS: [&str; 5] = [
    "The level rises and falls",
    "The sound travels from side to side",
    "A pattern of steps opens and closes the sound: the trance gate",
    "The pitch wavers",
    "Multiplied by a tone: metallic, robotic",
];
pub const TREMOLO: usize = 0;
pub const PAN: usize = 1;
pub const CUTTER: usize = 2;
pub const VIBRATO: usize = 3;
pub const RING: usize = 4;
pub const NOTES: [&str; 14] = [
    "1/32", "1/16T", "1/16", "1/16D", "1/8T", "1/8", "1/8D", "1/4T", "1/4", "1/4D", "1/2", "1 bar",
    "2 bars", "4 bars",
];
const NOTE_BEATS: [f32; 14] = [
    0.125,
    1.0 / 6.0,
    0.25,
    0.375,
    1.0 / 3.0,
    0.5,
    0.75,
    2.0 / 3.0,
    1.0,
    1.5,
    2.0,
    4.0,
    8.0,
    16.0,
];
pub const STEPS: [&str; 3] = ["4", "8", "16"];
pub const PATTERN: usize = 16;

pub mod p {
    pub const MODE: usize = 0;
    pub const SYNC: usize = 1;
    pub const NOTE: usize = 2;
    pub const RATE: usize = 3;
    pub const DEPTH: usize = 4;
    pub const SHAPE: usize = 5;
    pub const PHASE: usize = 6;
    pub const SMOOTH: usize = 7;
    pub const STEPS: usize = 8;
    pub const GATE: usize = 9;
    pub const FREQ: usize = 10;
    pub const MIX: usize = 11;
    pub const OUTPUT: usize = 12;
    pub const FIRST_STEP: usize = 13;
}
const COUNT: usize = p::FIRST_STEP + PATTERN;

const PARAMS: [ParamInfo; COUNT] = [
    choice("mode", "Mode", &MODES, 0.0),
    switch("sync", "Sync to tempo", 1.0),
    choice("note", "Note", &NOTES, 5.0),
    param("rate", "Rate", 0.05, 40.0, 4.0, "Hz"),
    param("depth", "Depth", 0.0, 100.0, 60.0, "%"),
    choice("shape", "Shape", &crate::filter::SHAPES, 0.0),
    param("phase", "Stereo phase", 0.0, 180.0, 0.0, "°"),
    param("smooth", "Smooth", 0.0, 100.0, 15.0, "%"),
    choice("steps", "Steps", &STEPS, 2.0),
    param("gate", "Gate length", 5.0, 100.0, 60.0, "%"),
    param("freq", "Frequency", 20.0, 4_000.0, 220.0, "Hz"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
    switch("s1", "Step 1", 1.0),
    switch("s2", "Step 2", 0.0),
    switch("s3", "Step 3", 1.0),
    switch("s4", "Step 4", 0.0),
    switch("s5", "Step 5", 1.0),
    switch("s6", "Step 6", 1.0),
    switch("s7", "Step 7", 0.0),
    switch("s8", "Step 8", 1.0),
    switch("s9", "Step 9", 1.0),
    switch("s10", "Step 10", 0.0),
    switch("s11", "Step 11", 1.0),
    switch("s12", "Step 12", 0.0),
    switch("s13", "Step 13", 1.0),
    switch("s14", "Step 14", 1.0),
    switch("s15", "Step 15", 1.0),
    switch("s16", "Step 16", 0.0),
];

/// Most a vibrato sways the delay, either way (ms at full depth).
const VIBRATO_MS: f32 = 2.0;

#[derive(Debug)]
pub struct Modulator {
    rate: f32,
    values: [f32; COUNT],
    bpm: f64,
    /// Where the modulation is in its cycle (0 to 1), and the step count of the cutter.
    phase: f32,
    step: f32,
    held: [f32; 2],
    noise: u32,
    /// The smoothed modulation per side.
    smoothed: [f32; 2],
    carrier: f32,
    lines: [Line; 2],
    shown: [f32; 3],
}

impl Default for Modulator {
    fn default() -> Self {
        let mut m = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            bpm: 120.0,
            phase: 0.0,
            step: 0.0,
            held: [0.0; 2],
            noise: 0x9e37_79b9,
            smoothed: [1.0; 2],
            carrier: 0.0,
            lines: [Line::new(16), Line::new(16)],
            shown: [1.0, 0.0, 0.0],
        };
        m.allocate();
        m
    }
}

/// Seconds of one cycle (or one cutter step) for these settings at this tempo.
pub fn period(values: &[f32], bpm: f64) -> f32 {
    if values[p::SYNC] >= 0.5 {
        let note = values[p::NOTE].round().clamp(0.0, (NOTES.len() - 1) as f32) as usize;
        NOTE_BEATS[note] * 60.0 / bpm.max(1.0) as f32
    } else {
        1.0 / values[p::RATE].max(0.01)
    }
}

impl Modulator {
    fn allocate(&mut self) {
        let frames = (VIBRATO_MS * 3.0 * 0.001 * self.rate) as usize + 8;
        self.lines = [Line::new(frames), Line::new(frames)];
    }

    fn mode(&self) -> usize {
        self.values[p::MODE]
            .round()
            .clamp(0.0, (MODES.len() - 1) as f32) as usize
    }

    #[inline]
    fn random(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

impl Effect for Modulator {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let v = self.values;
        let mode = self.mode();
        let depth = v[p::DEPTH] / 100.0;
        let shape = v[p::SHAPE].round().max(0.0) as usize;
        let offset = v[p::PHASE] / 360.0;
        let step = 1.0 / (period(&v, self.bpm) * self.rate).max(1.0);
        let steps = [4.0, 8.0, 16.0][v[p::STEPS].round().clamp(0.0, 2.0) as usize];
        let gate = v[p::GATE] / 100.0;
        let smooth_ms = 0.3 + v[p::SMOOTH] / 100.0 * 30.0;
        let smoothing = 1.0 - (-1.0 / (smooth_ms * 0.001 * self.rate)).exp();
        let carrier_step = v[p::FREQ] / self.rate;
        let wet = v[p::MIX] / 100.0;
        let output = db_to_gain(v[p::OUTPUT]);
        let pattern: [bool; PATTERN] = std::array::from_fn(|k| v[p::FIRST_STEP + k] >= 0.5);
        let vibrato = VIBRATO_MS * 0.001 * self.rate;
        let tau = std::f32::consts::TAU;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let dry = [*l, *r];
            self.phase += step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.held = [self.random(), self.random()];
                self.step = (self.step + 1.0) % steps;
            }
            let side_phase = [self.phase, (self.phase + offset) % 1.0];
            let lfo = [
                lfo_value(shape, side_phase[0], self.held[0]),
                lfo_value(shape, side_phase[1], self.held[1]),
            ];
            let out = match mode {
                PAN => {
                    let target = depth * lfo[0];
                    self.smoothed[0] += smoothing * (target - self.smoothed[0]);
                    let angle = (self.smoothed[0] + 1.0) * std::f32::consts::FRAC_PI_4;
                    let root = std::f32::consts::SQRT_2;
                    [dry[0] * angle.cos() * root, dry[1] * angle.sin() * root]
                }
                CUTTER => {
                    let index = self.step as usize % PATTERN;
                    let open = pattern[index] && self.phase < gate;
                    let target = if open { 1.0 } else { 1.0 - depth };
                    self.smoothed[0] += smoothing * (target - self.smoothed[0]);
                    [dry[0] * self.smoothed[0], dry[1] * self.smoothed[0]]
                }
                VIBRATO => {
                    let mut out = [0.0; 2];
                    for ch in 0..2 {
                        self.lines[ch].push(dry[ch]);
                        let delay = vibrato * (1.25 + depth * lfo[ch]);
                        out[ch] = self.lines[ch].read(delay.max(1.0));
                    }
                    out
                }
                RING => {
                    self.carrier = (self.carrier + carrier_step) % 1.0;
                    let tone = (self.carrier * tau).sin();
                    let g = 1.0 - depth + depth * tone;
                    [dry[0] * g, dry[1] * g]
                }
                _ => {
                    let mut out = [0.0; 2];
                    for ch in 0..2 {
                        let target = 1.0 - depth * (1.0 - (lfo[ch] + 1.0) / 2.0);
                        self.smoothed[ch] += smoothing * (target - self.smoothed[ch]);
                        out[ch] = dry[ch] * self.smoothed[ch];
                    }
                    out
                }
            };
            *l = (dry[0] + wet * (out[0] - dry[0])) * output;
            *r = (dry[1] + wet * (out[1] - dry[1])) * output;
        }
        self.shown = [self.smoothed[0], self.step, self.phase];
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.step = 0.0;
        self.smoothed = [1.0, 1.0];
        if self.mode() == PAN {
            self.smoothed = [0.0, 0.0];
        }
        self.carrier = 0.0;
        for line in &mut self.lines {
            line.clear();
        }
    }

    /// The gain (or pan) now, the cutter's step and the cycle's phase.
    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out.iter_mut().zip(self.shown) {
            *o = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    fn modulator(settings: &[(&str, f32)]) -> Modulator {
        let mut m = Modulator::default();
        m.prepare(48_000, 512);
        m.set_tempo(120.0);
        for (id, v) in settings {
            m.set_param(index(id), *v);
        }
        m
    }

    /// Envelope of a steady 1 kHz tone through the modulator: the peak of each 5 ms.
    fn envelope(m: &mut Modulator, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * 48_000.0) as usize;
        let mut l: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            m.process(a, b);
        }
        let peaks = |x: &[f32]| {
            x.chunks(240)
                .map(|c| c.iter().fold(0.0f32, |p, s| p.max(s.abs())))
                .collect()
        };
        (peaks(&l), peaks(&r))
    }

    #[test]
    fn tremolo_follows_the_tempo() {
        // A 1/8 at 120 BPM: four dips a second.
        let mut m = modulator(&[("depth", 100.0)]);
        let (l, _) = envelope(&mut m, 2.0);
        let low = l.iter().cloned().fold(1.0f32, f32::min);
        assert!(low < 0.05, "{low}");
        let dips = l.windows(2).filter(|w| w[0] >= 0.25 && w[1] < 0.25).count();
        assert!((7..=9).contains(&dips), "{dips}");
    }

    #[test]
    fn autopan_moves_between_the_sides() {
        let mut m = modulator(&[("mode", PAN as f32), ("depth", 100.0)]);
        let (l, r) = envelope(&mut m, 1.0);
        let left_alone = l.iter().zip(&r).any(|(a, b)| *a > 0.6 && *b < 0.1);
        let right_alone = l.iter().zip(&r).any(|(a, b)| *b > 0.6 && *a < 0.1);
        assert!(left_alone && right_alone);
    }

    #[test]
    fn the_cutter_plays_its_pattern() {
        let mut settings = vec![
            ("mode", CUTTER as f32),
            ("depth", 100.0),
            ("note", 8.0),
            ("steps", 0.0),
            ("gate", 100.0),
        ];
        let ids: Vec<String> = (1..=16).map(|k| format!("s{k}")).collect();
        for (k, id) in ids.iter().enumerate() {
            settings.push((id.as_str(), if k == 1 { 0.0 } else { 1.0 }));
        }
        let mut m = modulator(&settings);
        // Quarter notes at 120 BPM: the second of every four (0.5 to 1 s) is silent.
        let (l, _) = envelope(&mut m, 2.0);
        let at = |s: f32| l[(s * 200.0) as usize];
        assert!(
            at(0.25) > 0.4 && at(0.75) < 0.02 && at(1.25) > 0.4,
            "{} {} {}",
            at(0.25),
            at(0.75),
            at(1.25)
        );
        assert!(at(2.75 - 2.0 + 1.0) > 0.4);
    }

    #[test]
    fn ring_and_vibrato_change_the_sound_and_mix_brings_it_back() {
        // Fully ringed, nothing is left at the tone's own frequency.
        let mut m = modulator(&[("mode", RING as f32), ("depth", 100.0)]);
        let n = 24_000;
        let tone: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let (mut l, mut r) = (tone.clone(), tone.clone());
        m.process(&mut l, &mut r);
        let at_1k = |x: &[f32]| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in x.iter().enumerate() {
                let a = std::f32::consts::TAU * 1000.0 * i as f32 / 48_000.0;
                re += v * a.cos();
                im += v * a.sin();
            }
            (re * re + im * im).sqrt()
        };
        assert!(at_1k(&l) < at_1k(&tone) * 0.02);
        let mut m = modulator(&[("mode", RING as f32), ("depth", 100.0), ("mix", 0.0)]);
        let (l, _) = envelope(&mut m, 0.5);
        assert!(l[10..].iter().all(|v| (*v - 0.5).abs() < 0.01));
        let mut m = modulator(&[("mode", VIBRATO as f32), ("depth", 100.0)]);
        let (l, _) = envelope(&mut m, 0.5);
        assert!(l[10..].iter().all(|v| (*v - 0.5).abs() < 0.03));
    }
}
