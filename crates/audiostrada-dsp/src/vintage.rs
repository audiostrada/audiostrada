//! Vintage compressor with three characters: Bus (a VCA bus compressor with auto release),
//! Opto (an optical leveler: feedback detection, soft knee, program-dependent release) and
//! FET (fast, punchy and colored, with an "all buttons" ratio). The color stage saturates
//! with each model's harmonics, oversampled.

use crate::compressor::gain_computer;
use crate::drive::{Oversampler, TAPS, kernel};
use crate::{
    Effect, ParamInfo, choice, db_to_gain, gain_to_db, param, pole, switch, time_coefficient,
};

const MODELS: [&str; 3] = ["Bus", "Opto", "FET"];

const PARAMS: [ParamInfo; 10] = [
    choice("model", "Model", &MODELS, 0.0),
    param("threshold", "Threshold", -40.0, 0.0, -16.0, "dB"),
    param("ratio", "Ratio", 1.5, 20.0, 4.0, ":1"),
    param("attack", "Attack", 0.05, 30.0, 10.0, "ms"),
    param("release", "Release", 50.0, 1200.0, 300.0, "ms"),
    switch("auto", "Auto release", 1.0),
    param("makeup", "Makeup", 0.0, 24.0, 3.0, "dB"),
    param("color", "Color", 0.0, 100.0, 30.0, "%"),
    param("sc_hpf", "Sidechain HPF", 20.0, 500.0, 60.0, "Hz"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
];

#[derive(Debug)]
pub(crate) struct VintageCompressor {
    rate: f32,
    values: [f32; 10],
    /// Smoothed gain change in dB (zero or negative).
    envelope: f32,
    /// Slow side of the auto release and the opto cell's memory.
    slow: f32,
    /// Gain change applied at the last sample, in dB.
    current: f32,
    hpf: [(f32, f32); 2],
    kernel: [f32; TAPS],
    stages: [Oversampler; 2],
    reduction: f32,
    levels: [f32; 2],
}

impl Default for VintageCompressor {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            envelope: 0.0,
            slow: 0.0,
            current: 0.0,
            hpf: [(0.0, 0.0); 2],
            kernel: kernel(),
            stages: [Oversampler::new(), Oversampler::new()],
            reduction: 0.0,
            levels: [-120.0; 2],
        }
    }
}

/// Gain change in dB for a level over the threshold, per model.
fn curve(model: usize, over: f32, ratio: f32) -> f32 {
    match model {
        // Bus: firm 2 dB knee.
        0 => gain_computer(over, 0.0, ratio, 2.0),
        // Opto: very soft knee, ratio rising with the level up to the setting.
        1 => gain_computer(over, 0.0, ratio.min(10.0), 12.0),
        // FET: at the top of the range, "all buttons": the ratio overshoots on loud peaks.
        _ if ratio >= 19.5 => -over.max(0.0) * 1.02 - (over.max(0.0) * 0.05).powi(2),
        _ => gain_computer(over, 0.0, ratio, 4.0),
    }
}

/// The model's harmonic color for an amount 0 to 1 (unity gain for small signals).
fn color(model: usize, amount: f32, x: f32) -> f32 {
    if amount <= 0.0 {
        return x;
    }
    let drive = 1.0 + amount * 4.0;
    let shaped = match model {
        // Transformer: gentle symmetric soft clip.
        0 => (x * drive).tanh() / drive,
        // Tube: asymmetric, even harmonics.
        1 => {
            let bias = 0.15 * amount;
            ((x * drive + bias).tanh() - bias.tanh()) / drive
        }
        // FET: harder, odd harmonics.
        _ => {
            let y = x * drive;
            (y / (1.0 + y.abs().powf(2.5)).powf(0.4)) / drive
        }
    };
    x + (shaped - x) * amount.min(1.0)
}

impl Effect for VintageCompressor {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
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
        self.slow = 0.0;
        self.current = 0.0;
        self.hpf = [(0.0, 0.0); 2];
        self.stages = [Oversampler::new(), Oversampler::new()];
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

impl VintageCompressor {
    fn run(&mut self, left: &mut [f32], right: &mut [f32], key: Option<(&[f32], &[f32])>) {
        let [
            model,
            threshold,
            ratio,
            attack,
            release,
            auto,
            makeup,
            amount,
            sc_hpf,
            mix,
        ] = self.values;
        let model = model.round().clamp(0.0, 2.0) as usize;
        let rate = self.rate;
        let (attack_ms, release_ms) = match model {
            // The optical cell attacks in about 10 ms whatever the setting.
            1 => (10.0, 60.0),
            _ => (attack, release),
        };
        let att = time_coefficient(attack_ms, rate);
        let rel = time_coefficient(release_ms, rate);
        let slow_rel = time_coefficient(if model == 1 { 2500.0 } else { 1200.0 }, rate);
        let hp = pole(sc_hpf, rate);
        let auto = auto >= 0.5 && model == 0;
        let (makeup, wet) = (db_to_gain(makeup), mix / 100.0);
        let amount = amount / 100.0;
        let kernel = self.kernel;
        let mut deepest = 0.0f32;
        let (mut peak_in, mut peak_out) = (0.0f32, 0.0f32);
        for i in 0..left.len().min(right.len()) {
            let (l, r) = (left[i], right[i]);
            let (kl, kr) = match key {
                Some((a, b)) => (
                    a.get(i).copied().unwrap_or(0.0),
                    b.get(i).copied().unwrap_or(0.0),
                ),
                None => (l, r),
            };
            // Detector high-pass, so the lows do not pump the whole mix.
            let mut detected = 0.0f32;
            for (ch, x) in [kl, kr].into_iter().enumerate() {
                let (x1, y1) = self.hpf[ch];
                let y = hp * (y1 + x - x1);
                self.hpf[ch] = (x, y);
                detected = detected.max(y.abs());
            }
            peak_in = peak_in.max(l.abs().max(r.abs()));
            // Opto listens to its own output (feedback detection).
            let level = if model == 1 {
                gain_to_db(detected) + self.current
            } else {
                gain_to_db(detected)
            };
            let target = curve(model, level - threshold, ratio);
            // Two envelopes: Opto and Bus auto release blend a fast and a slow recovery, so
            // short peaks come back quickly and sustained compression slowly.
            for (env, rel) in [(&mut self.envelope, rel), (&mut self.slow, slow_rel)] {
                *env = if target < *env {
                    target + att * (*env - target)
                } else {
                    target + rel * (*env - target)
                };
            }
            let reduction = if model == 1 || auto {
                0.5 * (self.envelope + self.slow)
            } else {
                self.envelope
            };
            self.current = reduction;
            deepest = deepest.min(reduction);
            let gain = db_to_gain(reduction) * makeup;
            let mut out = [l * gain, r * gain];
            for (ch, s) in out.iter_mut().enumerate() {
                *s = if amount > 0.0 {
                    self.stages[ch].run(&kernel, *s, |x| color(model, amount, x))
                } else {
                    *s
                };
            }
            left[i] = l * (1.0 - wet) + out[0] * wet;
            right[i] = r * (1.0 - wet) + out[1] * wet;
            peak_out = peak_out.max(left[i].abs().max(right[i].abs()));
        }
        self.reduction = deepest;
        self.levels = [gain_to_db(peak_in), gain_to_db(peak_out)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steady(model: f32, color: f32, level: f32) -> (f32, f32) {
        let mut c = VintageCompressor::default();
        c.set_param(0, model);
        c.set_param(6, 0.0);
        c.set_param(7, color);
        c.set_param(8, 20.0);
        c.prepare(48_000, 512);
        let mut out = 0.0;
        for _ in 0..400 {
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..512)
                .map(|i| {
                    let s = level * (i as f32 * 0.05).sin();
                    (s, s)
                })
                .unzip();
            c.process(&mut l, &mut r);
            out = l.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        }
        let mut meter = [0.0; 3];
        c.meters(&mut meter);
        (gain_to_db(out), meter[0])
    }

    #[test]
    fn every_model_compresses_loud_and_leaves_quiet_alone() {
        for model in 0..3 {
            let (loud, reduction) = steady(model as f32, 0.0, 0.5);
            assert!(reduction < -3.0, "model {model}: {reduction}");
            assert!(loud < gain_to_db(0.5) - 2.0, "model {model}: {loud}");
            let (quiet, reduction) = steady(model as f32, 0.0, 0.01);
            assert!(reduction > -0.5, "model {model}: {reduction}");
            assert!(
                (quiet - gain_to_db(0.01)).abs() < 0.5,
                "model {model}: {quiet}"
            );
        }
    }

    #[test]
    fn color_adds_harmonics_and_stays_bounded() {
        for model in 0..3 {
            for x in [-2.0f32, -0.3, 0.0, 0.3, 2.0] {
                let y = color(model, 1.0, x);
                assert!(
                    y.is_finite() && y.abs() <= x.abs() + 0.05,
                    "{model} {x} {y}"
                );
            }
            assert!(
                (color(model, 1.0, 0.001) - 0.001).abs() < 1e-4,
                "unity for small"
            );
            assert!(color(model, 1.0, 1.0) < 0.98, "{model} saturates");
        }
        let (colored, _) = steady(2.0, 100.0, 0.5);
        assert!(colored.is_finite());
    }
}
