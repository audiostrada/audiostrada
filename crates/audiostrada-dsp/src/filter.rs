//! Multimode filter: a zero-delay-feedback state-variable filter (low-pass, high-pass,
//! band-pass, notch) with drive, swept by an LFO (free or synced) and an envelope follower.

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

const TYPES: [&str; 4] = ["Low-pass", "High-pass", "Band-pass", "Notch"];
const NOTES: [&str; 6] = ["1/16", "1/8", "1/4", "1/2", "1 bar", "2 bars"];
const NOTE_BEATS: [f32; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

const PARAMS: [ParamInfo; 11] = [
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
];

#[derive(Debug)]
pub struct Filter {
    rate: f32,
    values: [f32; 11],
    bpm: f64,
    /// Integrator states per channel.
    ic: [[f32; 2]; 2],
    phase: f32,
    envelope: f32,
    shown: f32,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            bpm: 120.0,
            ic: [[0.0; 2]; 2],
            phase: 0.0,
            envelope: 0.0,
            shown: 2000.0,
        }
    }
}

impl Effect for Filter {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
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
        ] = self.values;
        let kind = kind.round().clamp(0.0, 3.0) as usize;
        let lfo_hz = if sync >= 0.5 {
            let beats = NOTE_BEATS[(note.round().max(0.0) as usize).min(5)];
            self.bpm.max(1.0) as f32 / 60.0 / beats
        } else {
            lfo_rate
        };
        let step = lfo_hz / self.rate;
        let k = 2.0 - 1.95 * (resonance / 100.0);
        let drive = db_to_gain(drive);
        let (wet, out) = (mix / 100.0, db_to_gain(output));
        let env_a = crate::time_coefficient(5.0, self.rate);
        let env_r = crate::time_coefficient(150.0, self.rate);
        let nyquist = self.rate * 0.45;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let level = l.abs().max(r.abs());
            let c = if level > self.envelope { env_a } else { env_r };
            self.envelope = level + c * (self.envelope - level);
            self.phase = (self.phase + step).fract();
            let lfo = (self.phase * std::f32::consts::TAU).sin();
            // Octaves of sweep: up to 4 for the LFO, 5 for the envelope.
            let octaves = lfo * lfo_depth / 100.0 * 4.0
                + (self.envelope * 4.0).min(1.0) * envelope / 100.0 * 5.0;
            let fc = (cutoff * 2f32.powf(octaves)).clamp(20.0, nyquist);
            self.shown = fc;
            let g = (std::f32::consts::PI * fc / self.rate).tan();
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            let a3 = g * a2;
            for (ch, s) in [&mut *l, &mut *r].into_iter().enumerate() {
                let x = if drive > 1.0 {
                    (*s * drive).tanh() / drive.sqrt()
                } else {
                    *s
                };
                let [ic1, ic2] = &mut self.ic[ch];
                let v3 = x - *ic2;
                let v1 = a1 * *ic1 + a2 * v3;
                let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                *ic1 = 2.0 * v1 - *ic1;
                *ic2 = 2.0 * v2 - *ic2;
                let y = match kind {
                    0 => v2,
                    1 => x - k * v1 - v2,
                    2 => v1 * k,
                    _ => x - k * v1,
                };
                *s = (*s * (1.0 - wet) + y * wet) * out;
            }
        }
    }

    fn reset(&mut self) {
        self.ic = [[0.0; 2]; 2];
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
}
