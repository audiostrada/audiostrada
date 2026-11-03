//! Compressor: a clean modern one (feed-forward, stereo-linked, soft knee, peak detection) or
//! the Bus, Opto and FET characters of the vintage engine, with makeup, mix and a sidechain
//! input.

use crate::vintage::VintageCompressor;
use crate::{
    Effect, EffectInfo, ParamInfo, choice, db_to_gain, gain_to_db, key_peak, param, switch,
    time_coefficient,
};

pub const INFO: EffectInfo = EffectInfo {
    id: "compressor",
    name: "Strada-Comp",
    params: &UNIFIED,
    meters: &["Gain reduction", "Input", "Output"],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Clean, Bus, Opto and FET: transparent control, glue, smooth leveling or punch.",
};

/// Characters, in parameter order: the clean engine, then the vintage models.
pub const TYPES: [&str; 4] = ["Clean", "Bus", "Opto", "FET"];

/// What each character does.
pub const TYPE_HINTS: [&str; 4] = [
    "Clean and modern: transparent, with a soft knee",
    "VCA bus compressor: glue for mixes and groups, auto release",
    "Optical leveler: smooth, program-dependent, great on vocals and bass",
    "Fast and punchy with color; the top ratio is the 'all buttons' mode",
];

pub const CLEAN: usize = 0;
pub const OPTO: usize = 2;

const UNIFIED: [ParamInfo; 11] = [
    choice("type", "Character", &TYPES, 0.0),
    param("threshold", "Threshold", -60.0, 0.0, -18.0, "dB"),
    param("ratio", "Ratio", 1.0, 20.0, 4.0, ":1"),
    param("attack", "Attack", 0.05, 100.0, 10.0, "ms"),
    param("release", "Release", 10.0, 1200.0, 150.0, "ms"),
    switch("auto", "Auto release", 1.0),
    param("knee", "Knee", 0.0, 24.0, 6.0, "dB"),
    param("makeup", "Makeup", 0.0, 24.0, 0.0, "dB"),
    param("color", "Color", 0.0, 100.0, 30.0, "%"),
    param("sc_hpf", "Sidechain HPF", 20.0, 500.0, 20.0, "Hz"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
];

/// Whether the character `kind` uses the parameter `id` (the editor greys the others).
pub fn uses(kind: usize, id: &str) -> bool {
    match id {
        "knee" => kind == CLEAN,
        "color" | "sc_hpf" => kind != CLEAN,
        "auto" => kind == 1,
        // The optical cell sets its own times.
        "attack" | "release" => kind != OPTO,
        _ => true,
    }
}

/// The compressor insert: one engine or the other, with the same settings.
#[derive(Debug)]
pub struct Compressor {
    values: [f32; 11],
    clean: Clean,
    vintage: VintageCompressor,
}

impl Default for Compressor {
    fn default() -> Self {
        let mut c = Self {
            values: UNIFIED.map(|p| p.default),
            clean: Clean::default(),
            vintage: VintageCompressor::default(),
        };
        for index in 0..UNIFIED.len() {
            c.forward(index);
        }
        c
    }
}

impl Compressor {
    fn kind(&self) -> usize {
        (self.values[0].round().max(0.0) as usize).min(TYPES.len() - 1)
    }

    /// Hands a setting to the engine parameters it maps to.
    fn forward(&mut self, index: usize) {
        let v = self.values[index];
        let (c, vi) = (&mut self.clean, &mut self.vintage);
        // Indices in the clean engine and in the vintage one.
        let (clean, vintage) = match index {
            0 => (None, Some(0)),
            1 => (Some(0), Some(1)),
            2 => (Some(1), Some(2)),
            3 => (Some(2), Some(3)),
            4 => (Some(3), Some(4)),
            5 => (None, Some(5)),
            6 => (Some(4), None),
            7 => (Some(5), Some(6)),
            8 => (None, Some(7)),
            9 => (None, Some(8)),
            _ => (Some(6), Some(9)),
        };
        if let Some(i) = clean {
            c.set_param(i, v);
        }
        if let Some(i) = vintage {
            vi.set_param(i, if index == 0 { (v - 1.0).max(0.0) } else { v });
        }
    }
}

impl Effect for Compressor {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.clean.prepare(sample_rate, max_frames);
        self.vintage.prepare(sample_rate, max_frames);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let before = self.kind() == CLEAN;
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.forward(index);
        }
        if before != (self.kind() == CLEAN) {
            self.clean.reset();
            self.vintage.reset();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.kind() == CLEAN {
            self.clean.process(left, right);
        } else {
            self.vintage.process(left, right);
        }
    }

    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: (&[f32], &[f32])) {
        if self.kind() == CLEAN {
            self.clean.process_keyed(left, right, key);
        } else {
            self.vintage.process_keyed(left, right, key);
        }
    }

    fn reset(&mut self) {
        self.clean.reset();
        self.vintage.reset();
    }

    fn meters(&self, out: &mut [f32]) {
        if self.kind() == CLEAN {
            self.clean.meters(out);
        } else {
            self.vintage.meters(out);
        }
    }
}

const PARAMS: [ParamInfo; 7] = [
    param("threshold", "Threshold", -60.0, 0.0, -18.0, "dB"),
    param("ratio", "Ratio", 1.0, 20.0, 4.0, ":1"),
    param("attack", "Attack", 0.1, 100.0, 10.0, "ms"),
    param("release", "Release", 10.0, 1000.0, 120.0, "ms"),
    param("knee", "Knee", 0.0, 24.0, 6.0, "dB"),
    param("makeup", "Makeup", 0.0, 24.0, 0.0, "dB"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
];

/// Static curve: gain change in dB (zero or negative) for an input level in dB.
pub fn gain_computer(level: f32, threshold: f32, ratio: f32, knee: f32) -> f32 {
    let over = level - threshold;
    let slope = 1.0 / ratio.max(1.0) - 1.0;
    if knee > 0.0 && 2.0 * over.abs() <= knee {
        slope * (over + knee / 2.0).powi(2) / (2.0 * knee)
    } else if over > 0.0 {
        slope * over
    } else {
        0.0
    }
}

/// The clean engine, also the Amp's compressor pedal.
#[derive(Debug)]
pub(crate) struct Clean {
    rate: f32,
    values: [f32; 7],
    attack: f32,
    release: f32,
    /// Smoothed gain change in dB.
    envelope: f32,
    reduction: f32,
    /// Peaks of the last block, in dB.
    levels: [f32; 2],
}

impl Default for Clean {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            attack: 0.0,
            release: 0.0,
            envelope: 0.0,
            reduction: 0.0,
            levels: [-120.0; 2],
        }
    }
}

impl Clean {
    fn update_times(&mut self) {
        self.attack = time_coefficient(self.values[2], self.rate);
        self.release = time_coefficient(self.values[3], self.rate);
    }
}

impl Effect for Clean {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.update_times();
        self.envelope = 0.0;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.update_times();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.run(left, right, None);
    }

    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: (&[f32], &[f32])) {
        self.run(left, right, Some(key));
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out
            .iter_mut()
            .zip([self.reduction, self.levels[0], self.levels[1]])
        {
            *o = v;
        }
    }
}

impl Clean {
    /// Compresses `left`/`right`, detecting on `key` when given, else on the signal itself.
    fn run(&mut self, left: &mut [f32], right: &mut [f32], key: Option<(&[f32], &[f32])>) {
        let [threshold, ratio, _, _, knee, makeup, mix] = self.values;
        let (makeup, wet) = (db_to_gain(makeup), mix / 100.0);
        let mut deepest: f32 = 0.0;
        let (mut peak_in, mut peak_out) = (0.0f32, 0.0f32);
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let detected = key_peak(key, i, *l, *r);
            peak_in = peak_in.max(detected);
            let level = gain_to_db(detected);
            let target = gain_computer(level, threshold, ratio, knee);
            let coefficient = if target < self.envelope {
                self.attack
            } else {
                self.release
            };
            self.envelope = target + coefficient * (self.envelope - target);
            deepest = deepest.min(self.envelope);
            let gain = db_to_gain(self.envelope) * makeup;
            let factor = 1.0 - wet + wet * gain;
            *l *= factor;
            *r *= factor;
            peak_out = peak_out.max(l.abs().max(r.abs()));
        }
        self.reduction = deepest;
        self.levels = [gain_to_db(peak_in), gain_to_db(peak_out)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_curve_and_steady_state() {
        assert_eq!(gain_computer(-30.0, -18.0, 4.0, 0.0), 0.0);
        assert!((gain_computer(-6.0, -18.0, 4.0, 0.0) + 9.0).abs() < 1e-4);
        assert!(
            gain_computer(-18.0, -18.0, 4.0, 6.0) < 0.0,
            "soft knee starts below"
        );
        let mut c = Clean::default();
        c.prepare(48_000, 512);
        // -6 dBFS square wave: 12 dB over at 4:1 gives -9 dB.
        let (mut l, mut r) = (vec![0.5f32; 512], vec![-0.5f32; 512]);
        for _ in 0..200 {
            l.fill(0.5);
            r.fill(-0.5);
            c.process(&mut l, &mut r);
        }
        assert!(
            (gain_to_db(l[511]) - (-6.02 - 9.0)).abs() < 0.2,
            "{}",
            gain_to_db(l[511])
        );
        let mut meter = [0.0];
        c.meters(&mut meter);
        assert!((meter[0] + 9.0).abs() < 0.2);
    }

    #[test]
    fn the_key_drives_the_reduction_in_every_character() {
        for (kind, name) in TYPES.iter().enumerate() {
            let mut c = Compressor::default();
            c.set_param(0, kind as f32);
            c.set_param(1, -30.0);
            c.prepare(48_000, 512);
            let mut out = 0.0;
            for block in 0..100 {
                let key: Vec<f32> = (0..512)
                    .map(|i| 0.5 * ((block * 512 + i) as f32 * 0.13).sin())
                    .collect();
                let (mut l, mut r) = (vec![0.01f32; 512], vec![0.01f32; 512]);
                c.process_keyed(&mut l, &mut r, (&key, &key));
                out = l[511];
            }
            assert!(out < 0.01 * 0.7, "{name}: {out}");
            let mut meters = [0.0; 3];
            c.meters(&mut meters);
            assert!(meters[0] < -3.0, "{name}");
        }
    }

    #[test]
    fn switching_engine_starts_from_rest_and_uses_follows_the_character() {
        let mut c = Compressor::default();
        c.set_param(1, -40.0);
        c.prepare(48_000, 256);
        let (mut l, mut r) = (vec![0.5f32; 256], vec![0.5f32; 256]);
        c.process(&mut l, &mut r);
        assert!(c.clean.envelope < -1.0);
        c.set_param(0, OPTO as f32);
        assert_eq!(c.clean.envelope, 0.0, "the clean engine was reset");
        assert!(uses(CLEAN, "knee") && !uses(OPTO, "knee"));
        assert!(!uses(OPTO, "attack") && uses(CLEAN, "release"));
        assert!(uses(1, "auto") && !uses(3, "auto") && uses(2, "sc_hpf"));
        assert!(uses(3, "threshold"));
    }

    #[test]
    fn every_character_compresses_and_reports() {
        for (kind, name) in TYPES.iter().enumerate() {
            let mut c = Compressor::default();
            c.set_param(0, kind as f32);
            c.set_param(1, -30.0);
            c.prepare(48_000, 512);
            let mut out = 0.0;
            for block in 0..200 {
                // A sine: the vintage detector's high-pass would remove a constant.
                let mut l: Vec<f32> = (0..512)
                    .map(|i| 0.5 * ((block * 512 + i) as f32 * 0.13).sin())
                    .collect();
                let mut r = l.clone();
                c.process(&mut l, &mut r);
                out = l.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            }
            assert!(out < 0.25 && out.is_finite(), "{name}: {out}");
            let mut meter = [0.0];
            c.meters(&mut meter);
            assert!(meter[0] < -3.0, "{name} shows its reduction: {}", meter[0]);
        }
    }
}
