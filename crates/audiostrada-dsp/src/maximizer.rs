//! Maximizer: a look-ahead brickwall limiter for mastering. Gain pushes the level into the
//! ceiling; inter-sample peaks are estimated so the output stays under the ceiling in
//! true peak. The gain follows a sliding minimum smoothed over the look-ahead, so peaks are
//! caught without distortion, then recovers with a program-dependent release.

use crate::loudness::{Hold, LIMITER_METERS, LevelMeter, limiter_meters};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "maximizer",
    name: "Strada-Max",
    params: &PARAMS,
    meters: LIMITER_METERS,
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Mastering limiter: louder without clipping, true-peak ceiling, look-ahead.",
};

const STYLES: [&str; 3] = ["Transparent", "Punchy", "Loud"];

const PARAMS: [ParamInfo; 5] = [
    param("gain", "Gain", 0.0, 24.0, 4.0, "dB"),
    param("ceiling", "Ceiling", -12.0, 0.0, -1.0, "dB"),
    param("release", "Release", 1.0, 1000.0, 120.0, "ms"),
    choice("style", "Style", &STYLES, 0.0),
    param("link", "Stereo link", 0.0, 100.0, 100.0, "%"),
];

const LOOKAHEAD_MS: f32 = 1.5;

#[derive(Debug)]
pub struct Maximizer {
    rate: f32,
    values: [f32; 5],
    length: usize,
    /// Delayed audio and the required gains of the look-ahead window, per channel.
    delay: [Vec<f32>; 2],
    required: [Vec<f32>; 2],
    held: [Vec<f32>; 2],
    sums: [f64; 2],
    at: usize,
    delay_at: usize,
    /// Last samples per channel, for the inter-sample peak estimate.
    history: [[f32; 3]; 2],
    gain: [f32; 2],
    slow: [f32; 2],
    reduction: Hold,
    input_meter: LevelMeter,
    output_meter: LevelMeter,
}

impl Default for Maximizer {
    fn default() -> Self {
        let mut m = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            length: 1,
            delay: Default::default(),
            required: Default::default(),
            held: Default::default(),
            sums: [0.0; 2],
            at: 0,
            delay_at: 0,
            history: [[0.0; 3]; 2],
            gain: [1.0; 2],
            slow: [1.0; 2],
            reduction: Hold::new(48_000),
            input_meter: LevelMeter::new(48_000),
            output_meter: LevelMeter::new(48_000),
        };
        m.allocate();
        m
    }
}

/// Highest value of the Catmull-Rom curve through four samples between the middle two,
/// checked at three points: an estimate of the inter-sample peak.
#[inline]
fn intersample(p: [f32; 4]) -> f32 {
    let mut peak = p[1].abs().max(p[2].abs());
    for t in [0.25f32, 0.5, 0.75] {
        let t2 = t * t;
        let t3 = t2 * t;
        let v = 0.5
            * ((2.0 * p[1])
                + (-p[0] + p[2]) * t
                + (2.0 * p[0] - 5.0 * p[1] + 4.0 * p[2] - p[3]) * t2
                + (-p[0] + 3.0 * p[1] - 3.0 * p[2] + p[3]) * t3);
        peak = peak.max(v.abs());
    }
    peak
}

impl Maximizer {
    fn allocate(&mut self) {
        self.length = ((LOOKAHEAD_MS * 0.001 * self.rate) as usize).max(2);
        self.delay = [vec![0.0; self.length - 1], vec![0.0; self.length - 1]];
        self.delay_at = 0;
        self.required = [vec![1.0; self.length], vec![1.0; self.length]];
        self.held = [vec![1.0; self.length], vec![1.0; self.length]];
        self.sums = [self.length as f64; 2];
        self.at = 0;
        self.history = [[0.0; 3]; 2];
        self.gain = [1.0; 2];
        self.slow = [1.0; 2];
    }
}

impl Effect for Maximizer {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.reduction = Hold::new(sample_rate);
        self.input_meter = LevelMeter::new(sample_rate);
        self.output_meter = LevelMeter::new(sample_rate);
        self.allocate();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn latency(&self) -> u32 {
        (self.length - 1) as u32
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [gain, ceiling, release, style, link] = self.values;
        let style = style.round().clamp(0.0, 2.0) as usize;
        let input = db_to_gain(gain);
        let ceiling = db_to_gain(ceiling);
        let release_ms = match style {
            0 => release,
            1 => release * 0.5,
            _ => release * 0.3,
        };
        let rel = crate::time_coefficient(release_ms, self.rate);
        let slow_rel = crate::time_coefficient(release_ms * 8.0, self.rate);
        // Punchy and Loud shave the tallest peaks with a soft clipper before limiting.
        let clip = match style {
            0 => 0.0,
            1 => 0.3,
            _ => 0.7,
        };
        let slow_weight = [0.5, 0.3, 0.0][style];
        let link = link / 100.0;
        let n = self.length;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.input_meter.push(*l, *r);
            let mut x = [*l * input, *r * input];
            let mut deepest = 1.0f32;
            if clip > 0.0 {
                for s in &mut x {
                    let knee = ceiling * (1.0 - 0.25 * clip);
                    if s.abs() > knee {
                        let over = (s.abs() - knee) / (ceiling * 2.0 - knee).max(1e-6);
                        *s = s.signum() * (knee + (ceiling * 2.0 - knee) * over.tanh() * 0.5);
                    }
                }
            }
            let mut needed = [1.0f32; 2];
            for ch in 0..2 {
                let h = self.history[ch];
                let peak = intersample([h[0], h[1], h[2], x[ch]]);
                self.history[ch] = [h[1], h[2], x[ch]];
                needed[ch] = if peak > ceiling { ceiling / peak } else { 1.0 };
            }
            let both = needed[0].min(needed[1]);
            for ch in 0..2 {
                let req = needed[ch] + (both - needed[ch]) * link;
                // Sliding minimum over the window, then a moving average of it.
                self.required[ch][self.at] = req;
                let held = self.required[ch].iter().fold(1.0f32, |m, &g| m.min(g));
                self.sums[ch] += f64::from(held) - f64::from(self.held[ch][self.at]);
                self.held[ch][self.at] = held;
                let target = (self.sums[ch] / n as f64) as f32;
                // Fast and slow recovery blended: transients recover fast, dense
                // passages slowly, without pumping.
                self.gain[ch] = if target < self.gain[ch] {
                    target
                } else {
                    target + rel * (self.gain[ch] - target)
                };
                self.slow[ch] = if target < self.slow[ch] {
                    target
                } else {
                    target + slow_rel * (self.slow[ch] - target)
                };
                // Both envelopes stay under the target, so their blend does too.
                let g = self.gain[ch] + (self.slow[ch] - self.gain[ch]) * slow_weight;
                let delayed = self.delay[ch][self.delay_at];
                self.delay[ch][self.delay_at] = x[ch];
                let out = (delayed * g).clamp(-ceiling, ceiling);
                x[ch] = out;
                deepest = deepest.min(g);
            }
            self.at = (self.at + 1) % n;
            self.delay_at = (self.delay_at + 1) % (n - 1);
            *l = x[0];
            *r = x[1];
            self.reduction.push(1.0 - deepest);
            self.output_meter.push(*l, *r);
        }
    }

    // Runs on the audio thread after jumps: clears in place, never reallocates.
    fn reset(&mut self) {
        for ch in 0..2 {
            self.delay[ch].fill(0.0);
            self.required[ch].fill(1.0);
            self.held[ch].fill(1.0);
        }
        self.sums = [self.length as f64; 2];
        self.at = 0;
        self.delay_at = 0;
        self.history = [[0.0; 3]; 2];
        self.gain = [1.0; 2];
        self.slow = [1.0; 2];
        self.reduction.reset();
        self.input_meter.reset();
        self.output_meter.reset();
    }

    fn meters(&self, out: &mut [f32]) {
        limiter_meters(out, &self.reduction, &self.input_meter, &self.output_meter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_stays_under_the_ceiling_and_gets_louder() {
        for style in 0..3 {
            let mut m = Maximizer::default();
            m.set_param(0, 12.0);
            m.set_param(1, -1.0);
            m.set_param(3, style as f32);
            m.prepare(48_000, 512);
            let ceiling = db_to_gain(-1.0);
            let mut rms_in = 0.0;
            let mut rms_out = 0.0;
            for block in 0..200 {
                let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..512)
                    .map(|i| {
                        let t = (block * 512 + i) as f32 / 48_000.0;
                        let s = 0.3 * (t * 220.0 * std::f32::consts::TAU).sin()
                            + 0.2 * (t * 3520.0 * std::f32::consts::TAU).sin()
                            + if (block * 512 + i) % 9600 == 0 {
                                0.4
                            } else {
                                0.0
                            };
                        (s, s * 0.8)
                    })
                    .unzip();
                rms_in += l.iter().map(|s| s * s).sum::<f32>();
                m.process(&mut l, &mut r);
                rms_out += l.iter().map(|s| s * s).sum::<f32>();
                assert!(
                    l.iter()
                        .chain(&r)
                        .all(|s| s.is_finite() && s.abs() <= ceiling + 1e-6),
                    "style {style}"
                );
            }
            assert!(
                rms_out > rms_in * 2.0,
                "style {style}: louder {}",
                rms_out / rms_in
            );
        }
    }

    #[test]
    fn meters_show_levels_reduction_and_loudness() {
        let mut m = Maximizer::default();
        m.set_param(0, 12.0);
        m.prepare(48_000, 480);
        for block in 0..400 {
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..480)
                .map(|i| {
                    let t = (block * 480 + i) as f32 / 48_000.0;
                    let s = 0.25 * (t * 997.0 * std::f32::consts::TAU).sin();
                    (s, s)
                })
                .unzip();
            m.process(&mut l, &mut r);
        }
        let mut out = [0.0; 8];
        m.meters(&mut out);
        let [reduction, peak, in_l, in_r, out_l, _, in_lufs, out_lufs] = out;
        assert!(
            (in_l + 12.0).abs() < 0.1 && (in_r - in_l).abs() < 1e-3,
            "{in_l}"
        );
        assert!(
            peak <= -1.0 + 1e-3 && peak > -1.5 && out_l == peak,
            "{peak}"
        );
        // 12 dB of gain into a -1 dB ceiling from -12 dB peaks: about 1 dB of reduction.
        assert!(reduction < -0.5 && reduction > -3.0, "{reduction}");
        assert!(out_lufs - in_lufs > 9.0, "{in_lufs} -> {out_lufs}");
    }

    #[test]
    fn quiet_material_passes_untouched_after_the_look_ahead() {
        let mut m = Maximizer::default();
        m.set_param(0, 0.0);
        m.prepare(48_000, 256);
        let latency = m.latency() as usize;
        let input: Vec<f32> = (0..2048).map(|i| 0.1 * (i as f32 * 0.03).sin()).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        m.process(&mut l, &mut r);
        for i in latency..2048 {
            assert!((l[i] - input[i - latency]).abs() < 1e-5, "{i}");
        }
    }
}
