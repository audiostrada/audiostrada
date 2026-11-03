//! Compressor: a clean modern character (feed-forward, soft knee, peak detection) or the Bus,
//! Opto and FET ones (their curves, times and oversampled color), with lookahead, hold, range,
//! a filtered and audible sidechain, stereo link, auto gain and an aligned dry mix.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::drive::{Oversampler, Taps};
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

const UNIFIED: [ParamInfo; 17] = [
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
    param("lookahead", "Lookahead", 0.0, MAX_LOOKAHEAD_MS, 0.0, "ms"),
    param("hold", "Hold", 0.0, 500.0, 0.0, "ms"),
    param("range", "Range", 0.0, 60.0, 60.0, "dB"),
    switch("auto_gain", "Auto gain", 0.0),
    param("link", "Stereo link", 0.0, 100.0, 100.0, "%"),
    switch("listen", "Sidechain listen", 0.0),
];

/// Longest lookahead.
pub const MAX_LOOKAHEAD_MS: f32 = 20.0;
/// Delay of the vintage characters' oversampled color stage, rounded to a sample.
pub const COLOR_LATENCY: u32 = crate::drive::LATENCY;

/// Indices into [`INFO`]'s parameters.
mod p {
    pub const TYPE: usize = 0;
    pub const ATTACK: usize = 3;
    pub const RELEASE: usize = 4;
    pub const SC_HPF: usize = 9;
    pub const LOOKAHEAD: usize = 11;
    pub const HOLD: usize = 12;
}

/// Whether the character `kind` uses the parameter `id` (the editor greys the others).
pub fn uses(kind: usize, id: &str) -> bool {
    match id {
        "knee" => kind == CLEAN,
        "color" => kind != CLEAN,
        // Program-dependent release: the clean engine and the bus model.
        "auto" => kind == CLEAN || kind == 1,
        // The optical cell sets its own times.
        "attack" | "release" => kind != OPTO,
        _ => true,
    }
}

/// The delay the compressor adds for these settings: its lookahead, and the color stage of
/// the vintage characters.
pub fn latency_of(values: &[f32], rate: f32) -> u32 {
    let kind = values.first().map_or(0, |v| v.round().max(0.0) as usize);
    let lookahead = values
        .get(p::LOOKAHEAD)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, MAX_LOOKAHEAD_MS);
    (lookahead * 0.001 * rate).round() as u32 + if kind == CLEAN { 0 } else { COLOR_LATENCY }
}

/// Gain change in dB a character's static curve gives at an input level, within the range.
pub fn reduction_at(kind: usize, level: f32, values: (f32, f32, f32), range: f32) -> f32 {
    let (threshold, ratio, knee) = values;
    let change = if kind == CLEAN {
        gain_computer(level, threshold, ratio, knee)
    } else {
        crate::vintage::curve(kind - 1, level - threshold, ratio)
    };
    change.max(-range)
}

/// Makeup that brings a compressed signal back near its level, from the settings only (so it
/// does not drift with the program): half of what the curve takes at full scale.
pub fn auto_makeup(threshold: f32, ratio: f32) -> f32 {
    (-threshold).max(0.0) * (1.0 - 1.0 / ratio.max(1.0)) * 0.5
}

/// The compressor insert: a detector (sidechain filter, stereo link, the character's curve and
/// times, hold, range), a lookahead delay, the vintage characters' oversampled color, makeup
/// and a dry mix kept aligned with the wet path.
#[derive(Debug)]
pub struct Compressor {
    values: [f32; 17],
    rate: f32,
    hpf: Coefficients,
    hpf_state: [Biquad; 2],
    /// Gain change per side in dB: fast and slow envelopes, the one applied, hold left.
    envelope: [f32; 2],
    slow: [f32; 2],
    current: [f32; 2],
    held: [usize; 2],
    attack: f32,
    release: f32,
    slow_release: f32,
    hold: usize,
    /// Lookahead delay of the audio, per side.
    line: [Vec<f32>; 2],
    at: usize,
    delay: usize,
    /// Wet and dry through the same oversampling, so they stay aligned in the mix.
    wet: [Oversampler; 2],
    dry: [Oversampler; 2],
    reduction: f32,
    levels: [f32; 2],
}

impl Default for Compressor {
    fn default() -> Self {
        let mut c = Self {
            values: UNIFIED.map(|p| p.default),
            rate: 48_000.0,
            hpf: Coefficients::IDENTITY,
            hpf_state: Default::default(),
            envelope: [0.0; 2],
            slow: [0.0; 2],
            current: [0.0; 2],
            held: [0; 2],
            attack: 0.0,
            release: 0.0,
            slow_release: 0.0,
            hold: 0,
            line: [Vec::new(), Vec::new()],
            at: 0,
            delay: 0,
            wet: [Oversampler::new(Taps::Long), Oversampler::new(Taps::Long)],
            dry: [Oversampler::new(Taps::Long), Oversampler::new(Taps::Long)],
            reduction: 0.0,
            levels: [-120.0; 2],
        };
        c.update();
        c
    }
}

impl Compressor {
    fn kind(&self) -> usize {
        (self.values[p::TYPE].round().max(0.0) as usize).min(TYPES.len() - 1)
    }

    /// Times, filter and delay from the settings.
    fn update(&mut self) {
        let kind = self.kind();
        let rate = self.rate;
        let (attack, release) = if kind == OPTO {
            // The optical cell attacks in about 10 ms and lets go in two stages.
            (10.0, 60.0)
        } else {
            (self.values[p::ATTACK], self.values[p::RELEASE])
        };
        self.attack = time_coefficient(attack, rate);
        self.release = time_coefficient(release, rate);
        self.slow_release = time_coefficient(if kind == OPTO { 2500.0 } else { 1200.0 }, rate);
        self.hold = (self.values[p::HOLD] * 0.001 * rate) as usize;
        let hz = self.values[p::SC_HPF];
        self.hpf = if hz > 20.5 {
            Coefficients::design(
                FilterKind::HighPass,
                f64::from(hz),
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
                f64::from(rate),
            )
        } else {
            Coefficients::IDENTITY
        };
        let lookahead = (self.values[p::LOOKAHEAD].clamp(0.0, MAX_LOOKAHEAD_MS) * 0.001 * rate)
            .round() as usize;
        self.delay = lookahead.min(self.line[0].len().saturating_sub(1));
    }

    fn clear(&mut self) {
        self.envelope = [0.0; 2];
        self.slow = [0.0; 2];
        self.current = [0.0; 2];
        self.held = [0; 2];
        self.hpf_state = Default::default();
        for line in &mut self.line {
            line.fill(0.0);
        }
        for stage in self.wet.iter_mut().chain(&mut self.dry) {
            stage.clear();
        }
    }

    fn run(&mut self, left: &mut [f32], right: &mut [f32], key: Option<(&[f32], &[f32])>) {
        let v = self.values;
        let kind = self.kind();
        let vintage = kind != CLEAN;
        let model = kind.saturating_sub(1);
        let (threshold, ratio, knee) = (v[1], v[2], v[6]);
        let two_stage = kind == OPTO || (v[5] >= 0.5 && (kind == CLEAN || kind == 1));
        let range = -v[13];
        let makeup = v[7]
            + if v[14] >= 0.5 {
                auto_makeup(threshold, ratio)
            } else {
                0.0
            };
        let makeup = db_to_gain(makeup);
        let (amount, wet, link, listen) =
            (v[8] / 100.0, v[10] / 100.0, v[15] / 100.0, v[16] >= 0.5);
        let size = self.line[0].len().max(1);
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
            // The detector hears the key through its high-pass.
            let heard = [
                self.hpf_state[0].process(&self.hpf, f64::from(kl)) as f32,
                self.hpf_state[1].process(&self.hpf, f64::from(kr)) as f32,
            ];
            peak_in = peak_in.max(l.abs().max(r.abs()));
            let both = heard[0].abs().max(heard[1].abs());
            for (ch, side) in heard.iter().enumerate() {
                let detected = link * both + (1.0 - link) * side.abs();
                let mut level = gain_to_db(detected);
                // The optical cell listens to its own output.
                if kind == OPTO {
                    level += self.current[ch];
                }
                let target = if vintage {
                    crate::vintage::curve(model, level - threshold, ratio)
                } else {
                    gain_computer(level, threshold, ratio, knee)
                }
                .max(range);
                let attacking = target < self.envelope[ch];
                if attacking {
                    self.held[ch] = self.hold;
                }
                let holding = !attacking && self.held[ch] > 0;
                if holding {
                    self.held[ch] -= 1;
                } else {
                    for (env, rel) in [
                        (&mut self.envelope[ch], self.release),
                        (&mut self.slow[ch], self.slow_release),
                    ] {
                        let c = if target < *env { self.attack } else { rel };
                        *env = target + c * (*env - target);
                    }
                }
                self.current[ch] = if two_stage {
                    0.5 * (self.envelope[ch] + self.slow[ch])
                } else {
                    self.envelope[ch]
                };
            }
            deepest = deepest.min(self.current[0].min(self.current[1]));
            // The audio comes out of the lookahead line, late by the delay.
            self.line[0][self.at] = l;
            self.line[1][self.at] = r;
            let read = (self.at + size - self.delay) % size;
            let delayed = [self.line[0][read], self.line[1][read]];
            self.at = (self.at + 1) % size;
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                let gain = db_to_gain(self.current[ch]) * makeup;
                let (mut processed, mut dry) = (delayed[ch] * gain, delayed[ch]);
                if vintage {
                    processed =
                        self.wet[ch].run(processed, |x| crate::vintage::color(model, amount, x));
                    dry = self.dry[ch].run(dry, |x| x);
                }
                out[ch] = if listen {
                    heard[ch]
                } else {
                    dry * (1.0 - wet) + processed * wet
                };
            }
            left[i] = out[0];
            right[i] = out[1];
            peak_out = peak_out.max(out[0].abs().max(out[1].abs()));
        }
        self.reduction = deepest;
        self.levels = [gain_to_db(peak_in), gain_to_db(peak_out)];
    }
}

impl Effect for Compressor {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        let size = (MAX_LOOKAHEAD_MS * 0.001 * self.rate).ceil() as usize + 2;
        self.line = [vec![0.0; size], vec![0.0; size]];
        self.at = 0;
        self.update();
        self.clear();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let before = self.kind();
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.update();
        }
        if before != self.kind() {
            self.clear();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.run(left, right, None);
    }

    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: (&[f32], &[f32])) {
        self.run(left, right, Some(key));
    }

    fn reset(&mut self) {
        self.clear();
    }

    fn latency(&self) -> u32 {
        latency_of(&self.values, self.rate)
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
    fn switching_character_starts_from_rest_and_uses_follows_it() {
        let mut c = Compressor::default();
        c.set_param(1, -40.0);
        c.prepare(48_000, 256);
        let (mut l, mut r) = (vec![0.5f32; 256], vec![0.5f32; 256]);
        c.process(&mut l, &mut r);
        assert!(c.envelope[0] < -1.0);
        c.set_param(0, OPTO as f32);
        assert_eq!(c.envelope[0], 0.0, "back at rest");
        assert!(uses(CLEAN, "knee") && !uses(OPTO, "knee"));
        assert!(!uses(OPTO, "attack") && uses(CLEAN, "release"));
        assert!(uses(1, "auto") && uses(CLEAN, "auto") && !uses(3, "auto"));
        assert!(uses(CLEAN, "sc_hpf") && uses(3, "threshold"));
    }

    fn index(id: &str) -> usize {
        INFO.params.iter().position(|p| p.id == id).unwrap()
    }

    fn sine(freq: f32, amplitude: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| amplitude * (i as f32 * freq * std::f32::consts::TAU / 48_000.0).sin())
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// Runs `input` (both sides) through in blocks; returns the left side.
    fn run(c: &mut Compressor, input: &[f32]) -> Vec<f32> {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            c.process(a, b);
        }
        l
    }

    #[test]
    fn the_dry_mix_stays_aligned_with_the_colored_path() {
        // A high tone well under the threshold, half wet: as loud as it came, no comb.
        let mut c = Compressor::default();
        c.set_param(0, 3.0);
        c.set_param(index("threshold"), 0.0);
        c.set_param(index("color"), 100.0);
        c.set_param(index("mix"), 50.0);
        c.prepare(48_000, 256);
        let input = sine(9_000.0, 0.01, 48_000);
        let out = run(&mut c, &input);
        let change = gain_to_db(rms(&out[4_800..]) / rms(&input[4_800..]));
        assert!(change.abs() < 0.3, "{change}");
        assert_eq!(c.latency(), COLOR_LATENCY);
    }

    #[test]
    fn lookahead_catches_the_attack_and_is_reported() {
        let burst: Vec<f32> = (0..9_600)
            .map(|i| {
                if i < 4_800 {
                    0.0
                } else {
                    0.9 * (i as f32 * 0.3).sin()
                }
            })
            .collect();
        let peak = |lookahead: f32| {
            let mut c = Compressor::default();
            c.set_param(index("threshold"), -30.0);
            c.set_param(index("ratio"), 20.0);
            c.set_param(index("attack"), 2.0);
            c.set_param(index("lookahead"), lookahead);
            c.prepare(48_000, 256);
            let out = run(&mut c, &burst);
            (out.iter().fold(0.0f32, |m, s| m.max(s.abs())), c.latency())
        };
        let (without, none) = peak(0.0);
        let (with, delay) = peak(5.0);
        assert_eq!((none, delay), (0, 240));
        assert!(with < without * 0.6, "{with} {without}");
    }

    #[test]
    fn range_hold_auto_gain_link_and_listen() {
        let loud = sine(1_000.0, 0.5, 24_000);
        // Range: never more than 6 dB taken.
        let mut c = Compressor::default();
        c.set_param(index("threshold"), -40.0);
        c.set_param(index("ratio"), 20.0);
        c.set_param(index("range"), 6.0);
        c.prepare(48_000, 256);
        run(&mut c, &loud);
        let mut meters = [0.0; 3];
        c.meters(&mut meters);
        assert!(meters[0] >= -6.1 && meters[0] < -5.0, "{}", meters[0]);
        // Auto gain brings the level back up.
        let level = |auto: f32| {
            let mut c = Compressor::default();
            c.set_param(index("threshold"), -30.0);
            c.set_param(index("auto_gain"), auto);
            c.prepare(48_000, 256);
            rms(&run(&mut c, &loud)[12_000..])
        };
        assert!(level(1.0) > level(0.0) * 2.0);
        // Unlinked, a quiet side is left alone by a loud one.
        let mut c = Compressor::default();
        c.set_param(index("threshold"), -30.0);
        c.set_param(index("link"), 0.0);
        c.prepare(48_000, 256);
        let quiet = sine(1_000.0, 0.005, 24_000);
        let (mut l, mut r) = (loud.clone(), quiet.clone());
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            c.process(a, b);
        }
        assert!((gain_to_db(rms(&r[12_000..]) / rms(&quiet[12_000..]))).abs() < 0.2);
        assert!(rms(&l[12_000..]) < rms(&loud[12_000..]) * 0.5);
        // Listening to the key: what its high-pass leaves of a low tone.
        let mut c = Compressor::default();
        c.set_param(index("sc_hpf"), 400.0);
        c.set_param(index("listen"), 1.0);
        c.prepare(48_000, 256);
        let low = sine(40.0, 0.5, 24_000);
        assert!(rms(&run(&mut c, &low)[12_000..]) < rms(&low) * 0.05);
        // Hold keeps the reduction after the signal stops.
        let mut c = Compressor::default();
        c.set_param(index("threshold"), -30.0);
        c.set_param(index("release"), 10.0);
        c.set_param(index("auto"), 0.0);
        c.set_param(index("hold"), 300.0);
        c.prepare(48_000, 256);
        run(&mut c, &loud);
        run(&mut c, &vec![0.0; 4_800]);
        assert!(c.envelope[0] < -10.0, "{}", c.envelope[0]);
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
