//! Console color: the input stage of an analog console's channel, or the summing of its buses.
//! Inspired by the published character of each console family (harmonics, transformer lows,
//! tone of the top end), not a model of their circuits. Calibrated on the working level: a
//! signal at -18 dBFS keeps its level, so switching the color on or off compares at equal
//! loudness.

use crate::biquad::{Biquad, Coefficients, FilterKind};

/// The consoles, in the order of the project model; the first colors nothing.
pub const MODELS: [&str; 5] = ["Clean", "Iron", "Imperial", "Discrete", "Velvet"];

/// What makes a console sound like itself, at an amount of 50 %.
#[derive(Clone, Copy, Debug)]
pub struct Voice {
    /// Gain into the saturation (1: about -60 dB of harmonics at the working level).
    pub drive: f32,
    /// Asymmetry: even harmonics.
    pub even: f32,
    /// Extra drive of the lows, below `split` Hz: a transformer's core.
    pub low_drive: f32,
    pub split: f64,
    /// Frequency and gain of a low shelf, a high shelf and a presence peak.
    pub low: (f64, f64),
    pub high: (f64, f64),
    pub presence: (f64, f64),
    /// Hiss level, dBFS.
    pub noise_db: f32,
}

pub fn voice(model: usize) -> Option<Voice> {
    match model {
        // Transformers: round lows, a soft and silky top.
        1 => Some(Voice {
            drive: 1.2,
            even: 0.08,
            low_drive: 1.7,
            split: 160.0,
            low: (70.0, 0.6),
            high: (16_000.0, -0.9),
            presence: (3_000.0, 0.0),
            noise_db: -86.0,
        }),
        // Tight and precise: odd harmonics, a bit of air.
        2 => Some(Voice {
            drive: 1.0,
            even: 0.02,
            low_drive: 1.0,
            split: 120.0,
            low: (40.0, -0.3),
            high: (11_000.0, 0.6),
            presence: (3_500.0, 0.2),
            noise_db: -90.0,
        }),
        // Discrete amplifiers: the most harmonics, lows and attack forward.
        3 => Some(Voice {
            drive: 1.45,
            even: 0.1,
            low_drive: 1.3,
            split: 140.0,
            low: (100.0, 0.5),
            high: (14_000.0, 0.0),
            presence: (2_500.0, 0.5),
            noise_db: -88.0,
        }),
        // Smooth and warm, very low distortion.
        4 => Some(Voice {
            drive: 0.7,
            even: 0.07,
            low_drive: 1.2,
            split: 200.0,
            low: (220.0, 0.4),
            high: (12_000.0, -0.6),
            presence: (3_000.0, 0.0),
            noise_db: -92.0,
        }),
        _ => None,
    }
}

/// A soft saturation of gain `g` and asymmetry `bias`, of slope 1 at zero: small signals pass
/// unchanged, louder ones get rounder.
#[inline]
pub fn saturate(x: f32, g: f32, bias: f32) -> f32 {
    if g < 1e-3 {
        return x;
    }
    let offset = (g * bias).tanh();
    ((g * (x + bias)).tanh() - offset) / (g * (1.0 - offset * offset))
}

/// The coloring of one channel or bus.
#[derive(Debug)]
pub struct ConsoleColor {
    rate: f64,
    model: usize,
    amount: f32,
    bus: bool,
    noise: bool,
    voice: Option<Voice>,
    /// Drive of the band above and below the split.
    g: f32,
    low_g: f32,
    split: Coefficients,
    split_state: [Biquad; 2],
    tone: [Coefficients; 3],
    tone_state: [[Biquad; 3]; 2],
    /// DC blocker after the asymmetric saturation, and the crosstalk's high pass.
    dc: Coefficients,
    dc_state: [Biquad; 2],
    bleed: Coefficients,
    bleed_state: [Biquad; 2],
    seed: u32,
}

impl ConsoleColor {
    pub fn new(rate: u32) -> Self {
        let mut color = Self {
            rate: f64::from(rate.max(1)),
            model: 0,
            amount: 0.0,
            bus: false,
            noise: false,
            voice: None,
            g: 0.0,
            low_g: 0.0,
            split: Coefficients::IDENTITY,
            split_state: Default::default(),
            tone: [Coefficients::IDENTITY; 3],
            tone_state: Default::default(),
            dc: Coefficients::IDENTITY,
            dc_state: Default::default(),
            bleed: Coefficients::IDENTITY,
            bleed_state: Default::default(),
            seed: 0x9e37_79b9,
        };
        color.design();
        color
    }

    /// The console (index into [`MODELS`]), its amount (0 to 100 %), summing a bus rather than
    /// a channel's input, and its hiss and crosstalk heard.
    pub fn set(&mut self, model: usize, amount: f32, bus: bool, noise: bool) {
        if (model, amount, bus, noise) != (self.model, self.amount, self.bus, self.noise) {
            let was = self.voice.is_some();
            (self.model, self.amount, self.bus, self.noise) = (model, amount, bus, noise);
            self.design();
            if !was {
                self.reset();
            }
        }
    }

    /// Changes something.
    pub fn active(&self) -> bool {
        self.voice.is_some()
    }

    fn design(&mut self) {
        self.voice = voice(self.model).filter(|_| self.amount > 0.0);
        let Some(v) = self.voice else {
            return;
        };
        let rate = self.rate;
        // 50 % is the console as it is, 100 % pushed twice as hard; a bus drives less.
        let scale = (self.amount / 50.0).clamp(0.0, 2.0);
        let bus = if self.bus { 0.7 } else { 1.0 };
        self.g = v.drive * scale * bus;
        self.low_g = self.g * v.low_drive;
        self.split = Coefficients::first_order(false, v.split, rate);
        let tone = f64::from(scale);
        let nyquist = rate * 0.45;
        self.tone = [
            Coefficients::design(FilterKind::LowShelf, v.low.0, 0.7, v.low.1 * tone, rate),
            Coefficients::design(
                FilterKind::HighShelf,
                v.high.0.min(nyquist),
                0.7,
                v.high.1 * tone,
                rate,
            ),
            Coefficients::design(
                FilterKind::Peak,
                v.presence.0.min(nyquist),
                0.8,
                v.presence.1 * tone,
                rate,
            ),
        ];
        self.dc = Coefficients::first_order(true, 5.0, rate);
        self.bleed = Coefficients::first_order(true, 1_000.0, rate);
    }

    pub fn reset(&mut self) {
        for s in self
            .split_state
            .iter_mut()
            .chain(self.dc_state.iter_mut())
            .chain(self.bleed_state.iter_mut())
            .chain(self.tone_state.iter_mut().flatten())
        {
            s.reset();
        }
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let Some(v) = self.voice else {
            return;
        };
        let bias = v.even;
        for (side, buffer) in [&mut *left, &mut *right].into_iter().enumerate() {
            for x in buffer.iter_mut() {
                let input = f64::from(*x);
                let low = self.split_state[side].process(&self.split, input) as f32;
                let high = *x - low;
                let mut y =
                    f64::from(saturate(low, self.low_g, bias) + saturate(high, self.g, bias * 0.5));
                y = self.dc_state[side].process(&self.dc, y);
                for (c, s) in self.tone.iter().zip(self.tone_state[side].iter_mut()) {
                    y = s.process(c, y);
                }
                *x = y as f32;
            }
        }
        if !self.noise {
            return;
        }
        let hiss = crate::db_to_gain(v.noise_db) * 1.7;
        // Summing: a little of each side leaks into the other, mostly the highs.
        let bleed = if self.bus { 0.006 } else { 0.0 };
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (dry_l, dry_r) = (*l, *r);
            if bleed > 0.0 {
                let into_l = self.bleed_state[0].process(&self.bleed, f64::from(dry_r)) as f32;
                let into_r = self.bleed_state[1].process(&self.bleed, f64::from(dry_l)) as f32;
                *l += into_l * bleed;
                *r += into_r * bleed;
            }
            *l += self.random() * hiss;
            *r += self.random() * hiss;
        }
    }
}

/// Levels of the 2nd to 6th harmonics of a 200 Hz tone at `level_db` through a console, in dB
/// below the fundamental: its fingerprint.
pub fn harmonics(model: usize, amount: f32, level_db: f32) -> [f32; 5] {
    const RATE: u32 = 48_000;
    const PERIOD: usize = 240;
    let mut color = ConsoleColor::new(RATE);
    color.set(model, amount, false, false);
    let a = crate::db_to_gain(level_db);
    let tone: Vec<f32> = (0..PERIOD * 40)
        .map(|i| a * (i as f32 * std::f32::consts::TAU / PERIOD as f32).sin())
        .collect();
    let (mut l, mut r) = (tone.clone(), tone);
    color.process(&mut l, &mut r);
    let tail = &l[PERIOD * 20..];
    let amplitude = |k: usize| {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &s) in tail.iter().enumerate() {
            let w = std::f64::consts::TAU * (k * i) as f64 / PERIOD as f64;
            re += f64::from(s) * w.cos();
            im += f64::from(s) * w.sin();
        }
        (re * re + im * im).sqrt()
    };
    let fundamental = amplitude(1).max(1e-12);
    let mut out = [0.0; 5];
    for (k, o) in out.iter_mut().enumerate() {
        *o = (20.0 * (amplitude(k + 2) / fundamental).max(1e-9).log10()) as f32;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gain_to_db;

    fn sine(freq: f32, db: f32) -> Vec<f32> {
        let a = crate::db_to_gain(db);
        (0..48_000)
            .map(|i| a * (i as f32 * freq * std::f32::consts::TAU / 48_000.0).sin())
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    fn through(model: usize, amount: f32, freq: f32, db: f32) -> (Vec<f32>, Vec<f32>) {
        let mut color = ConsoleColor::new(48_000);
        color.set(model, amount, false, false);
        let input = sine(freq, db);
        let mut l = input.clone();
        let mut r = input.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            color.process(a, b);
        }
        (input, l)
    }

    #[test]
    fn clean_or_none_changes_nothing() {
        for (model, amount) in [(0, 100.0), (1, 0.0)] {
            let (input, output) = through(model, amount, 440.0, -6.0);
            assert_eq!(input, output);
        }
    }

    #[test]
    fn the_working_level_keeps_its_loudness() {
        for (model, name) in MODELS.iter().enumerate().skip(1) {
            for amount in [50.0, 100.0] {
                let (input, output) = through(model, amount, 1_000.0, -18.0);
                let change = gain_to_db(rms(&output[24_000..]) / rms(&input[24_000..]));
                assert!(change.abs() < 0.75, "{name} {amount}: {change}");
            }
        }
    }

    #[test]
    fn each_console_has_its_fingerprint() {
        let iron = harmonics(1, 50.0, -12.0);
        let imperial = harmonics(2, 50.0, -12.0);
        let discrete = harmonics(3, 50.0, -12.0);
        let velvet = harmonics(4, 50.0, -12.0);
        // Transformers: even harmonics; the tight one: odd ones above the even.
        assert!(iron[0] > imperial[0] + 10.0, "{iron:?} {imperial:?}");
        assert!(imperial[1] > imperial[0], "{imperial:?}");
        // The discrete one is the richest, the smooth one the cleanest.
        let total = |h: [f32; 5]| h.iter().map(|d| 10f32.powf(d / 10.0)).sum::<f32>();
        assert!(total(discrete) > total(velvet) * 4.0);
        assert!(total(velvet) < total(imperial));
        // Within what a console does at the working level: below -30 dB.
        for h in [iron, imperial, discrete, velvet] {
            assert!(h.iter().all(|&d| d < -30.0), "{h:?}");
        }
        // More amount, more harmonics.
        assert!(harmonics(1, 100.0, -12.0)[0] > iron[0] + 3.0);
    }

    #[test]
    fn hiss_and_crosstalk_only_when_asked() {
        let mut color = ConsoleColor::new(48_000);
        color.set(1, 50.0, true, false);
        let (mut l, mut r) = (vec![0.0f32; 4_800], vec![0.0f32; 4_800]);
        color.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|&s| s == 0.0));
        color.set(1, 50.0, true, true);
        color.process(&mut l, &mut r);
        let level = gain_to_db(rms(&l));
        assert!((-95.0..-80.0).contains(&level), "{level}");
        // A signal on the left only leaks into the right.
        let mut quiet = ConsoleColor::new(48_000);
        quiet.set(2, 50.0, true, true);
        let mut l = sine(5_000.0, -6.0);
        let mut r = vec![0.0; l.len()];
        quiet.process(&mut l, &mut r);
        let leak = gain_to_db(rms(&r[24_000..]));
        assert!((-60.0..-35.0).contains(&leak), "{leak}");
    }
}
