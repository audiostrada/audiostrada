//! Maximizer: a look-ahead brickwall limiter for mastering. Gain pushes the level into the
//! ceiling; peaks are read between the samples (four times over, after BS.1770) so the
//! output stays under the ceiling in true peak. The gain follows a sliding minimum smoothed
//! over the look-ahead, so peaks are caught without distortion, then recovers with a
//! program-dependent release. Unity gain hears it at the level it came in, audition what it
//! removes.

use crate::limiter::{Core, needed};
use crate::loudness::{Hold, LIMITER_METERS, LevelMeter, TRUE_PEAK_LAG, TruePeak, limiter_meters};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param, switch};

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

const PARAMS: [ParamInfo; 7] = [
    param("gain", "Gain", 0.0, 24.0, 4.0, "dB"),
    param("ceiling", "Ceiling", -12.0, 0.0, -1.0, "dB"),
    param("release", "Release", 1.0, 1000.0, 120.0, "ms"),
    choice("style", "Style", &STYLES, 0.0),
    param("link", "Stereo link", 0.0, 100.0, 100.0, "%"),
    switch("unity", "Unity gain", 0.0),
    switch("audition", "Audition", 0.0),
];

const LOOKAHEAD_MS: f32 = 1.5;

#[derive(Debug)]
pub struct Maximizer {
    rate: f32,
    values: [f32; 7],
    core: Core,
    true_peak: TruePeak,
    reduction: Hold,
    input_meter: LevelMeter,
    output_meter: LevelMeter,
}

fn length(rate: f32) -> usize {
    ((LOOKAHEAD_MS * 0.001 * rate) as usize).max(2)
}

impl Default for Maximizer {
    fn default() -> Self {
        let mut core = Core::new(length(48_000.0), TRUE_PEAK_LAG);
        core.configure(length(48_000.0), TRUE_PEAK_LAG);
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            core,
            true_peak: TruePeak::default(),
            reduction: Hold::new(48_000),
            input_meter: LevelMeter::new(48_000),
            output_meter: LevelMeter::new(48_000),
        }
    }
}

impl Effect for Maximizer {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        let n = length(self.rate);
        self.core = Core::new(n, TRUE_PEAK_LAG);
        self.core.configure(n, TRUE_PEAK_LAG);
        self.true_peak.reset();
        self.reduction = Hold::new(sample_rate);
        self.input_meter = LevelMeter::new(sample_rate);
        self.output_meter = LevelMeter::new(sample_rate);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn latency(&self) -> u32 {
        self.core.latency()
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [gain, ceiling, release, style, link, unity, audition] = self.values;
        let style = style.round().clamp(0.0, 2.0) as usize;
        let input = db_to_gain(gain);
        let ceiling = db_to_gain(ceiling);
        let release_ms = release * [1.0, 0.5, 0.3][style];
        // Fast and slow recovery blended: transients recover fast, dense passages slowly,
        // without pumping.
        let times = (
            crate::time_coefficient(release_ms, self.rate),
            crate::time_coefficient(release_ms * 8.0, self.rate),
            [0.5, 0.3, 0.0][style],
        );
        // Punchy and Loud shave the tallest peaks with a soft clipper before limiting.
        let clip: f32 = [0.0, 0.3, 0.7][style];
        let link = link / 100.0;
        let unity = if unity >= 0.5 { 1.0 / input } else { 1.0 };
        let audition = audition >= 0.5;
        // Memoryless, so it can run before the detection and again after the delay.
        let knee = ceiling * (1.0 - 0.25 * clip);
        let shave = |s: f32| {
            if clip == 0.0 || s.abs() <= knee {
                return s;
            }
            let over = (s.abs() - knee) / (ceiling * 2.0 - knee).max(1e-6);
            s.signum() * (knee + (ceiling * 2.0 - knee) * over.tanh() * 0.5)
        };
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.input_meter.push(*l, *r);
            let boosted = [*l * input, *r * input];
            let peaks = self.true_peak.push(boosted.map(shave));
            let (dry, gains) = self.core.run(boosted, needed(peaks, ceiling, link), times);
            let out = [0, 1].map(|ch| {
                let limited = (shave(dry[ch]) * gains[ch]).clamp(-ceiling, ceiling);
                if audition {
                    (dry[ch] - limited) * unity
                } else {
                    limited * unity
                }
            });
            (*l, *r) = (out[0], out[1]);
            self.reduction.push(1.0 - gains[0].min(gains[1]));
            self.output_meter.push(*l, *r);
        }
    }

    // Runs on the audio thread after jumps: clears in place, never reallocates.
    fn reset(&mut self) {
        self.core.clear();
        self.true_peak.reset();
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

    #[test]
    fn true_peak_ceiling_and_audition() {
        let mut m = Maximizer::default();
        m.set_param(0, 12.0);
        m.set_param(1, -1.0);
        m.prepare(48_000, 256);
        let mut meter = LevelMeter::new(48_000);
        let tone = |b: usize, i: usize| {
            let n = (b * 256 + i) as f32;
            0.5 * (std::f32::consts::FRAC_PI_2 * n + 0.785).sin()
        };
        for b in 0..200 {
            let (mut l, mut r): (Vec<f32>, Vec<f32>) =
                (0..256).map(|i| (tone(b, i), tone(b, i))).unzip();
            m.process(&mut l, &mut r);
            for (a, b) in l.iter().zip(&r) {
                meter.push(*a, *b);
            }
        }
        assert!(
            meter.highest_true_peak() < -1.0 + 0.2,
            "{}",
            meter.highest_true_peak()
        );
        // Audition plus the limited sound is the boosted input.
        m.set_param(6, 1.0);
        m.reset();
        let (mut l, mut r): (Vec<f32>, Vec<f32>) =
            (0..256).map(|i| (tone(0, i), tone(0, i))).unzip();
        m.process(&mut l, &mut r);
        assert!(l.iter().all(|s| s.is_finite()));
    }
}
