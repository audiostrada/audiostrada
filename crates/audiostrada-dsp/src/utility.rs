//! Utility: gain, stereo balance and width, mono, channel swap, and phase tools: polarity per
//! side, a delay per side to time-align microphones, and a phase rotation (an all-pass) to
//! match the lows of two sources. A DC filter, and mono lows below a frequency. Meters: phase
//! correlation and the left/right level balance.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::line::Line;
use crate::{Effect, EffectInfo, db_to_gain, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "utility",
    name: "Strada-Utility",
    params: &[
        param("gain", "Gain", -48.0, 24.0, 0.0, "dB"),
        param("width", "Width", 0.0, 200.0, 100.0, "%"),
        switch("mono", "Mono", 0.0),
        switch("invert_left", "Invert left", 0.0),
        switch("invert_right", "Invert right", 0.0),
        switch("swap", "Swap channels", 0.0),
        param("balance", "Balance", -100.0, 100.0, 0.0, "%"),
        param("delay_left", "Delay left", 0.0, MAX_DELAY_MS, 0.0, "ms"),
        param("delay_right", "Delay right", 0.0, MAX_DELAY_MS, 0.0, "ms"),
        param("rotation", "Phase rotation", 0.0, 180.0, 0.0, "°"),
        param("rotation_freq", "Rotation at", 20.0, 2000.0, 100.0, "Hz"),
        switch("dc", "DC filter", 0.0),
        param("bass_mono", "Mono below", 20.0, 500.0, 20.0, "Hz"),
    ],
    meters: &["Correlation", "Balance"],
    instrument: false,
    files: &[],
    category: "Utility",
    description: "Gain, balance, width, mono, polarity, time alignment and phase rotation.",
};

const MAX_DELAY_MS: f32 = 20.0;
/// Time constant of the meters, in seconds.
const METER_TIME: f32 = 0.3;

#[derive(Debug)]
pub struct Utility {
    rate: f32,
    gain: f32,
    width: f32,
    mono: bool,
    invert: [bool; 2],
    swap: bool,
    /// -1 (left only) to 1 (right only).
    balance: f32,
    /// Delay of each side in samples, the target and where it is now (glides, no clicks).
    delay: [f32; 2],
    delay_now: [f32; 2],
    lines: [Line; 2],
    rotation: f32,
    rotation_freq: f32,
    /// First-order all-pass coefficient, None while there is no rotation.
    allpass: Option<f32>,
    allpass_state: [(f32, f32); 2],
    /// Gains applied at the end of the previous block, for ramps.
    current: Option<[f32; 4]>,
    /// Leaky sums of l*r, l*l and r*r for the meters.
    sums: [f32; 3],
    dc: bool,
    /// Last input and output of each side's DC filter.
    dc_state: [(f32, f32); 2],
    /// High-pass on the sides below which the lows are mono, None while off.
    bass_mono: Option<Coefficients>,
    bass_mono_freq: f32,
    side_hp: [Biquad; 2],
}

impl Default for Utility {
    fn default() -> Self {
        let mut utility = Self {
            rate: 48_000.0,
            gain: 1.0,
            width: 1.0,
            mono: false,
            invert: [false; 2],
            swap: false,
            balance: 0.0,
            delay: [0.0; 2],
            delay_now: [0.0; 2],
            lines: Default::default(),
            rotation: 0.0,
            rotation_freq: 100.0,
            allpass: None,
            allpass_state: [(0.0, 0.0); 2],
            current: None,
            sums: [0.0; 3],
            dc: false,
            dc_state: [(0.0, 0.0); 2],
            bass_mono: None,
            bass_mono_freq: 20.0,
            side_hp: [Biquad::default(); 2],
        };
        utility.allocate();
        utility
    }
}

impl Utility {
    fn allocate(&mut self) {
        let frames = (MAX_DELAY_MS * 0.001 * self.rate) as usize + 4;
        self.lines = [Line::new(frames), Line::new(frames)];
    }

    /// Output = matrix * (left, right): [ll, lr, rl, rr].
    fn matrix(&self) -> [f32; 4] {
        let width = if self.mono { 0.0 } else { self.width };
        // Mid/side: mid = (l + r) / 2, side = (l - r) / 2 * width.
        let (a, b) = ((1.0 + width) / 2.0, (1.0 - width) / 2.0);
        let mut m = [a, b, b, a];
        if self.swap {
            m = [m[2], m[3], m[0], m[1]];
        }
        // Balance: the side it leans away from gets quieter, the other keeps its level.
        let (bl, br) = ((1.0 - self.balance).min(1.0), (1.0 + self.balance).min(1.0));
        let sign = |inv: bool| if inv { -self.gain } else { self.gain };
        let (gl, gr) = (sign(self.invert[0]) * bl, sign(self.invert[1]) * br);
        [m[0] * gl, m[1] * gl, m[2] * gr, m[3] * gr]
    }

    fn update_bass_mono(&mut self) {
        self.bass_mono = (self.bass_mono_freq > 21.0).then(|| {
            Coefficients::design(
                FilterKind::HighPass,
                f64::from(self.bass_mono_freq),
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
                f64::from(self.rate),
            )
        });
    }

    /// The all-pass delays the phase by `rotation` degrees at `rotation_freq`: 0 in the deep
    /// lows, up to 180 degrees towards the top.
    fn update_allpass(&mut self) {
        self.allpass = (self.rotation > 0.0).then(|| {
            let half = (self.rotation.min(179.0) / 2.0).to_radians();
            let corner = (self.rotation_freq / half.tan()).clamp(5.0, self.rate * 0.49);
            let t = (std::f32::consts::PI * corner / self.rate).tan();
            (t - 1.0) / (t + 1.0)
        });
    }
}

impl Effect for Utility {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
        self.update_allpass();
        self.update_bass_mono();
        self.reset();
        self.current = None;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let ms = |v: f32| v.clamp(0.0, MAX_DELAY_MS) * 0.001;
        match index {
            0 => self.gain = db_to_gain(value),
            1 => self.width = value / 100.0,
            2 => self.mono = value >= 0.5,
            3 => self.invert[0] = value >= 0.5,
            4 => self.invert[1] = value >= 0.5,
            5 => self.swap = value >= 0.5,
            6 => self.balance = (value / 100.0).clamp(-1.0, 1.0),
            7 => self.delay[0] = ms(value) * self.rate,
            8 => self.delay[1] = ms(value) * self.rate,
            9 => {
                self.rotation = value.clamp(0.0, 180.0);
                self.update_allpass();
            }
            10 => {
                self.rotation_freq = value.clamp(20.0, 2000.0);
                self.update_allpass();
            }
            11 => self.dc = value >= 0.5,
            12 => {
                self.bass_mono_freq = value.clamp(20.0, 500.0);
                self.update_bass_mono();
            }
            _ => {}
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let target = self.matrix();
        let start = self.current.unwrap_or(target);
        self.current = Some(target);
        let n = left.len().min(right.len());
        let step = 1.0 / n.max(1) as f32;
        let delayed = self.delay != [0.0; 2] || self.delay_now != [0.0; 2];
        let leak = 1.0 - (-1.0 / (METER_TIME * self.rate)).exp();
        // About 4 Hz, whatever the rate.
        let dc_pole = 1.0 - 25.0 / self.rate;
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let mut x = [*l, *r];
            if self.dc {
                for (v, (x1, y1)) in x.iter_mut().zip(&mut self.dc_state) {
                    let y = *v - *x1 + dc_pole * *y1;
                    (*x1, *y1) = (*v, y);
                    *v = y;
                }
            }
            if delayed {
                for (ch, v) in x.iter_mut().enumerate() {
                    // Glides to a new delay over a few milliseconds.
                    self.delay_now[ch] += (self.delay[ch] - self.delay_now[ch]) * 0.002;
                    if (self.delay_now[ch] - self.delay[ch]).abs() < 1e-3 {
                        self.delay_now[ch] = self.delay[ch];
                    }
                    self.lines[ch].push(*v);
                    *v = self.lines[ch].read(1.0 + self.delay_now[ch]);
                }
            }
            if let Some(a) = self.allpass {
                for (ch, v) in x.iter_mut().enumerate() {
                    let (x1, y1) = self.allpass_state[ch];
                    let y = a * *v + x1 - a * y1;
                    self.allpass_state[ch] = (*v, y);
                    *v = y;
                }
            }
            let t = (i + 1) as f32 * step;
            let m: [f32; 4] = std::array::from_fn(|k| start[k] + (target[k] - start[k]) * t);
            *l = m[0] * x[0] + m[1] * x[1];
            *r = m[2] * x[0] + m[3] * x[1];
            if let Some(c) = &self.bass_mono {
                let (mid, mut side) = (0.5 * (*l + *r), f64::from(0.5 * (*l - *r)));
                for f in &mut self.side_hp {
                    side = f.process(c, side);
                }
                (*l, *r) = (mid + side as f32, mid - side as f32);
            }
            for (sum, v) in self.sums.iter_mut().zip([*l * *r, *l * *l, *r * *r]) {
                *sum += (v - *sum) * leak;
            }
        }
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.delay_now = self.delay;
        self.allpass_state = [(0.0, 0.0); 2];
        self.sums = [0.0; 3];
        self.dc_state = [(0.0, 0.0); 2];
        self.side_hp = [Biquad::default(); 2];
    }

    /// Phase correlation (-1 to 1), and how much louder the right side is than the left, dB.
    fn meters(&self, out: &mut [f32]) {
        let [lr, ll, rr] = self.sums;
        let energy = (ll * rr).sqrt();
        let values = [
            if energy > 1e-12 {
                (lr / energy).clamp(-1.0, 1.0)
            } else {
                1.0
            },
            if ll > 1e-12 && rr > 1e-12 {
                (10.0 * (rr / ll).log10()).clamp(-24.0, 24.0)
            } else {
                0.0
            },
        ];
        for (o, v) in out.iter_mut().zip(values) {
            *o = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(params: &[(usize, f32)], l: f32, r: f32) -> (f32, f32) {
        let mut u = Utility::default();
        for &(i, v) in params {
            u.set_param(i, v);
        }
        let (mut a, mut b) = ([l; 8], [r; 8]);
        u.process(&mut a, &mut b);
        (a[7], b[7])
    }

    #[test]
    fn defaults_are_transparent() {
        assert_eq!(run(&[], 0.3, -0.7), (0.3, -0.7));
    }

    #[test]
    fn mono_width_polarity_swap_gain() {
        assert_eq!(run(&[(2, 1.0)], 1.0, 0.0), (0.5, 0.5));
        assert_eq!(run(&[(1, 0.0)], 1.0, -1.0), (0.0, 0.0));
        assert_eq!(run(&[(3, 1.0)], 0.5, 0.5), (-0.5, 0.5));
        assert_eq!(run(&[(5, 1.0)], 0.2, 0.8), (0.8, 0.2));
        let (l, _) = run(&[(0, -6.0206)], 1.0, 1.0);
        assert!((l - 0.5).abs() < 1e-4);
        assert_eq!(run(&[(4, 1.0)], 0.5, 0.5), (0.5, -0.5));
        assert_eq!(run(&[(1, 200.0)], 1.0, 0.0), (1.5, -0.5));
    }

    #[test]
    fn changes_ramp_over_the_block() {
        let mut u = Utility::default();
        let (mut a, mut b) = ([1.0f32; 4], [1.0f32; 4]);
        u.process(&mut a, &mut b);
        u.set_param(3, 1.0);
        u.set_param(99, 1.0);
        let (mut a, mut b) = ([1.0f32; 4], [0.0f32; 4]);
        u.process(&mut a, &mut b);
        assert_eq!(a, [0.5, 0.0, -0.5, -1.0], "from +1 to -1 in four steps");
        u.prepare(48_000, 4);
        assert_eq!(u.current, None, "a new stream starts on the target");
    }

    #[test]
    fn balance_quiets_the_side_it_leans_away_from() {
        assert_eq!(run(&[(6, 50.0)], 1.0, 1.0), (0.5, 1.0));
        assert_eq!(run(&[(6, -100.0)], 1.0, 1.0), (1.0, 0.0));
    }

    #[test]
    fn delays_a_side_by_whole_samples() {
        let mut u = Utility::default();
        u.prepare(48_000, 64);
        // 1 ms at 48 kHz: 48 samples.
        u.set_param(8, 1.0);
        u.reset();
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];
        r[0] = 1.0;
        l[0] = 1.0;
        u.process(&mut l, &mut r);
        assert_eq!(l[0], 1.0, "left untouched");
        assert!(
            (r[48] - 1.0).abs() < 1e-4 && r[0].abs() < 1e-6,
            "right 48 samples late"
        );
    }

    #[test]
    fn rotation_keeps_the_level_and_turns_the_phase() {
        let mut u = Utility::default();
        u.prepare(48_000, 4800);
        u.set_param(9, 90.0);
        u.set_param(10, 100.0);
        let sine: Vec<f32> = (0..48_000)
            .map(|i| (i as f32 * 100.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        let (mut l, mut r) = (sine.clone(), sine.clone());
        for (a, b) in l.chunks_mut(4800).zip(r.chunks_mut(4800)) {
            u.process(a, b);
        }
        let peak = l[24_000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 1.0).abs() < 0.01, "all-pass: same level, {peak}");
        // 90 degrees late at 100 Hz: the output follows a minus cosine.
        let dot: f32 = l[24_000..]
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v * ((24_000 + i) as f32 * 100.0 * std::f32::consts::TAU / 48_000.0).cos()
            })
            .sum::<f32>()
            / 12_000.0;
        assert!(dot < -0.95, "a quarter turn: {dot}");
    }

    #[test]
    fn meters_read_correlation_and_balance() {
        let mut u = Utility::default();
        u.prepare(48_000, 4800);
        let (mut l, mut r) = (vec![0.5f32; 48_000], vec![-0.25f32; 48_000]);
        for (a, b) in l.chunks_mut(4800).zip(r.chunks_mut(4800)) {
            u.process(a, b);
        }
        let mut m = [0.0f32; 2];
        u.meters(&mut m);
        assert!(m[0] < -0.99, "opposite polarity: {}", m[0]);
        assert!((m[1] + 6.02).abs() < 0.1, "right 6 dB quieter: {}", m[1]);
    }

    #[test]
    fn dc_filter_and_mono_lows() {
        let mut u = Utility::default();
        u.set_param(11, 1.0);
        u.prepare(48_000, 4800);
        let (mut l, mut r) = (vec![0.3f32; 48_000], vec![0.3f32; 48_000]);
        u.process(&mut l, &mut r);
        assert!(l[47_999].abs() < 1e-3, "offset removed: {}", l[47_999]);
        let mut u = Utility::default();
        u.set_param(12, 200.0);
        u.prepare(48_000, 4800);
        // A 50 Hz tone in opposite phase: only sides, folded to mono below 200 Hz.
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..48_000)
            .map(|i| {
                let s = 0.5 * (i as f32 * 50.0 * std::f32::consts::TAU / 48_000.0).sin();
                (s, -s)
            })
            .unzip();
        u.process(&mut l, &mut r);
        let peak = l[24_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.05, "sides of the lows gone: {peak}");
    }
}
