//! Multimode filter: a zero-delay-feedback state-variable filter (low-pass, high-pass,
//! band-pass, notch), 12 or 24 dB per octave, with drive, swept by an LFO (six shapes, free or
//! synced, its phase offset between the sides) and an envelope follower. The cutoff glides.

use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "filter",
    name: "Strada-Filter",
    params: &PARAMS,
    meters: &["Cutoff"],
    instrument: false,
    files: &[],
    category: "EQ & Filter",
    description: "Resonant sweeping filter with drive, LFO and envelope: from subtle to wild.",
};

pub const TYPES: [&str; 4] = ["Low-pass", "High-pass", "Band-pass", "Notch"];
pub const TYPE_HINTS: [&str; 4] = [
    "Keeps the lows: closing it muffles, opening it brightens",
    "Keeps the highs: thins the sound from the bottom",
    "Keeps a band around the cutoff: a telephone or a wah",
    "Takes a band out around the cutoff: a hollow, phasing sound",
];
const NOTES: [&str; 6] = ["1/16", "1/8", "1/4", "1/2", "1 bar", "2 bars"];
const NOTE_BEATS: [f32; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

pub const SLOPES: [&str; 2] = ["12 dB/oct", "24 dB/oct"];
pub const SHAPES: [&str; 6] = ["Sine", "Triangle", "Saw up", "Saw down", "Square", "Random"];
/// Glide of the cutoff knob, so that turning it never steps.
const GLIDE_MS: f32 = 10.0;

const PARAMS: [ParamInfo; COUNT] = [
    choice("type", "Type", &TYPES, 0.0),
    param("cutoff", "Cutoff", 20.0, 20000.0, 2000.0, "Hz"),
    param("resonance", "Resonance", 0.0, 100.0, 30.0, "%"),
    param("drive", "Drive", 0.0, 24.0, 0.0, "dB"),
    param("lfo_rate", "LFO rate", 0.02, 20.0, 1.0, "Hz"),
    param("lfo_depth", "LFO depth", 0.0, 100.0, 0.0, "%"),
    switch("sync", "Sync to tempo", 0.0),
    choice("note", "LFO note", &NOTES, 2.0),
    param("envelope", "Envelope", -100.0, 100.0, 0.0, "%"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
    choice("slope", "Slope", &SLOPES, 0.0),
    choice("lfo_shape", "LFO shape", &SHAPES, 0.0),
    param("lfo_phase", "LFO stereo phase", 0.0, 180.0, 0.0, "°"),
    param("env_attack", "Envelope attack", 0.5, 100.0, 5.0, "ms"),
    param("env_release", "Envelope release", 10.0, 2000.0, 150.0, "ms"),
];
const COUNT: usize = 16;

/// The LFO at `phase` (0 to 1) for shape `shape`, from -1 to 1; `held` is the random value
/// of the current cycle.
pub fn lfo_value(shape: usize, phase: f32, held: f32) -> f32 {
    match shape {
        1 => 1.0 - 4.0 * (phase - 0.5).abs(),
        2 => 2.0 * phase - 1.0,
        3 => 1.0 - 2.0 * phase,
        4 => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        5 => held,
        _ => (phase * std::f32::consts::TAU).sin(),
    }
}

#[derive(Debug)]
pub struct Filter {
    rate: f32,
    values: [f32; COUNT],
    bpm: f64,
    /// Integrator states per channel and stage.
    ic: [[[f32; 2]; 2]; 2],
    phase: f32,
    envelope: f32,
    shown: f32,
    /// The cutoff knob as it glides.
    cutoff: f32,
    /// Random value of each side's LFO cycle, and the noise it comes from.
    held: [f32; 2],
    noise: u32,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            bpm: 120.0,
            ic: [[[0.0; 2]; 2]; 2],
            phase: 0.0,
            envelope: 0.0,
            shown: 2000.0,
            cutoff: 2000.0,
            held: [0.0; 2],
            noise: 0x2545_f491,
        }
    }
}

impl Effect for Filter {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.cutoff = self.values[1];
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
        let [
            kind,
            cutoff,
            resonance,
            drive,
            lfo_rate,
            lfo_depth,
            sync,
            note,
            envelope,
            mix,
            output,
            slope,
            shape,
            lfo_phase,
            env_attack,
            env_release,
        ] = self.values;
        let kind = kind.round().clamp(0.0, 3.0) as usize;
        let stages = if slope >= 0.5 { 2 } else { 1 };
        let shape = (shape.round().max(0.0) as usize).min(SHAPES.len() - 1);
        let lfo_hz = if sync >= 0.5 {
            let beats = NOTE_BEATS[(note.round().max(0.0) as usize).min(5)];
            self.bpm.max(1.0) as f32 / 60.0 / beats
        } else {
            lfo_rate
        };
        let step = lfo_hz / self.rate;
        let offset = lfo_phase / 360.0;
        let k = 2.0 - 1.95 * (resonance / 100.0);
        // Two stages each as resonant would ring too much: the first stays flat (Butterworth).
        let k_first = if stages == 2 {
            std::f32::consts::SQRT_2
        } else {
            k
        };
        let drive = db_to_gain(drive);
        let (wet, out) = (mix / 100.0, db_to_gain(output));
        let env_a = crate::time_coefficient(env_attack, self.rate);
        let env_r = crate::time_coefficient(env_release, self.rate);
        let glide = crate::time_coefficient(GLIDE_MS, self.rate);
        let nyquist = self.rate * 0.45;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let level = l.abs().max(r.abs());
            let c = if level > self.envelope { env_a } else { env_r };
            self.envelope = level + c * (self.envelope - level);
            // Gliding in octaves, so that a sweep sounds even.
            self.cutoff = cutoff * (self.cutoff / cutoff).powf(glide);
            let before = self.phase;
            self.phase = (self.phase + step).fract();
            let sweep_env = (self.envelope * 4.0).min(1.0) * envelope / 100.0 * 5.0;
            for (ch, s) in [&mut *l, &mut *r].into_iter().enumerate() {
                let phase = (self.phase + offset * ch as f32).fract();
                let previous = (before + offset * ch as f32).fract();
                // A new random value each time the cycle starts again.
                if phase < previous {
                    self.noise ^= self.noise << 13;
                    self.noise ^= self.noise >> 17;
                    self.noise ^= self.noise << 5;
                    self.held[ch] = self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
                }
                let lfo = lfo_value(shape, phase, self.held[ch]);
                // Octaves of sweep: up to 4 for the LFO, 5 for the envelope.
                let octaves = lfo * lfo_depth / 100.0 * 4.0 + sweep_env;
                let fc = (self.cutoff * 2f32.powf(octaves)).clamp(20.0, nyquist);
                if ch == 0 {
                    self.shown = fc;
                }
                let g = (std::f32::consts::PI * fc / self.rate).tan();
                let x = if drive > 1.0 {
                    (*s * drive).tanh() / drive.sqrt()
                } else {
                    *s
                };
                let mut y = x;
                for stage in 0..stages {
                    let k = if stage + 1 == stages { k } else { k_first };
                    let a1 = 1.0 / (1.0 + g * (g + k));
                    let a2 = g * a1;
                    let a3 = g * a2;
                    let input = y;
                    let [ic1, ic2] = &mut self.ic[ch][stage];
                    let v3 = input - *ic2;
                    let v1 = a1 * *ic1 + a2 * v3;
                    let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                    *ic1 = 2.0 * v1 - *ic1;
                    *ic2 = 2.0 * v2 - *ic2;
                    y = match kind {
                        0 => v2,
                        1 => input - k * v1 - v2,
                        2 => v1 * k,
                        _ => input - k * v1,
                    };
                }
                *s = (*s * (1.0 - wet) + y * wet) * out;
            }
        }
    }

    fn reset(&mut self) {
        self.ic = [[[0.0; 2]; 2]; 2];
        self.envelope = 0.0;
    }

    fn meters(&self, out: &mut [f32]) {
        if let Some(o) = out.first_mut() {
            *o = self.shown;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(kind: f32, f: f32) -> f32 {
        let mut filter = Filter::default();
        filter.set_param(0, kind);
        filter.set_param(1, 1000.0);
        filter.set_param(2, 0.0);
        filter.prepare(48_000, 512);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..24_000)
            .map(|i| {
                let s = 0.5 * (i as f32 * f * std::f32::consts::TAU / 48_000.0).sin();
                (s, s)
            })
            .unzip();
        filter.process(&mut l, &mut r);
        l[12_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn each_type_passes_and_stops_the_right_band() {
        assert!(
            level(0.0, 100.0) > 0.45 && level(0.0, 10_000.0) < 0.02,
            "low-pass"
        );
        assert!(
            level(1.0, 100.0) < 0.02 && level(1.0, 10_000.0) > 0.45,
            "high-pass"
        );
        assert!(
            level(2.0, 1000.0) > 0.4 && level(2.0, 100.0) < 0.1,
            "band-pass"
        );
        assert!(
            level(3.0, 1000.0) < 0.05 && level(3.0, 100.0) > 0.45,
            "notch"
        );
    }

    #[test]
    fn full_resonance_and_modulation_stay_stable() {
        let mut filter = Filter::default();
        filter.set_param(2, 100.0);
        filter.set_param(3, 24.0);
        filter.set_param(5, 100.0);
        filter.set_param(4, 20.0);
        filter.set_param(8, 100.0);
        filter.prepare(48_000, 512);
        let (mut l, mut r) = (vec![0.0f32; 48_000], vec![0.0f32; 48_000]);
        l[0] = 1.0;
        l[24_000] = 1.0;
        filter.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 20.0));
    }

    fn steady(changes: &[(usize, f32)], f: f32) -> f32 {
        let mut filter = Filter::default();
        for &(i, v) in changes {
            filter.set_param(i, v);
        }
        filter.prepare(48_000, 512);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..24_000)
            .map(|i| {
                let s = 0.5 * (i as f32 * f * std::f32::consts::TAU / 48_000.0).sin();
                (s, s)
            })
            .unzip();
        filter.process(&mut l, &mut r);
        l[12_000..].iter().fold(0.0f32, |m, s| m.max(s.abs())) / 0.5
    }

    #[test]
    fn the_steep_slope_cuts_twice_as_much() {
        let gentle = steady(&[(1, 1000.0), (2, 0.0)], 4000.0);
        let steep = steady(&[(1, 1000.0), (2, 0.0), (11, 1.0)], 4000.0);
        let (g, s) = (crate::gain_to_db(gentle), crate::gain_to_db(steep));
        assert!((g + 24.0).abs() < 3.0, "12 dB/oct two octaves up: {g}");
        assert!((s + 48.0).abs() < 4.0, "24 dB/oct two octaves up: {s}");
    }

    #[test]
    fn lfo_shapes_and_cutoff_glide() {
        assert_eq!(lfo_value(1, 0.5, 0.0), 1.0);
        assert_eq!(lfo_value(2, 0.0, 0.0), -1.0);
        assert_eq!(lfo_value(3, 0.0, 0.0), 1.0);
        assert_eq!(lfo_value(4, 0.75, 0.0), -1.0);
        assert_eq!(lfo_value(5, 0.3, 0.42), 0.42);
        let mut filter = Filter::default();
        filter.prepare(48_000, 64);
        filter.set_param(1, 200.0);
        let (mut l, mut r) = (vec![0.0f32; 48], vec![0.0f32; 48]);
        filter.process(&mut l, &mut r);
        assert!(
            filter.cutoff > 1500.0,
            "glides, does not jump: {}",
            filter.cutoff
        );
        let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
        filter.process(&mut l, &mut r);
        assert!((filter.cutoff - 200.0).abs() < 1.0);
    }
}
