//! Spectrum analyzer: passes audio through and reports the level of 128 bands, log-spaced from
//! 20 Hz to 20 kHz, from a windowed FFT of the chosen channels, falling back slowly after peaks.
//! The view, range, tilt and peak hold only change how the editor draws it.

use crate::{Effect, EffectInfo, ParamInfo, choice, param, switch};

pub const BANDS: usize = 128;
const SIZE: usize = 4096;
const HOP: usize = 1024;
const LOW: f32 = 20.0;
const HIGH: f32 = 20_000.0;

pub const INFO: EffectInfo = EffectInfo {
    id: "analyzer",
    name: "Analyzer",
    params: &PARAMS,
    meters: &[""; BANDS],
    instrument: false,
    files: &[],
    category: "Utility",
    description: "Spectrum analyzer.",
};

pub const VIEWS: [&str; 3] = ["Curve", "Bars", "Spectrogram"];
pub const SOURCES: [&str; 4] = ["Stereo", "Left", "Right", "Side"];
pub const RANGES: [&str; 3] = ["60 dB", "90 dB", "120 dB"];

const PARAMS: [ParamInfo; 6] = [
    param("decay", "Fall speed", 1.0, 60.0, 20.0, "dB/s"),
    choice("view", "View", &VIEWS, 0.0),
    choice("source", "Source", &SOURCES, 0.0),
    choice("range", "Range", &RANGES, 1.0),
    param("tilt", "Tilt", 0.0, 6.0, 0.0, "dB/oct"),
    switch("hold", "Peak hold", 1.0),
];

/// Lowest level shown by a range choice.
pub fn range_floor(range: f32) -> f32 {
    match range.round() as i32 {
        0 => -60.0,
        2 => -120.0,
        _ => -90.0,
    }
}

/// Center frequency of a band.
pub fn band_frequency(band: usize) -> f32 {
    LOW * (HIGH / LOW).powf((band as f32 + 0.5) / BANDS as f32)
}

/// In-place iterative radix-2 FFT of `re`/`im` (length a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -std::f32::consts::TAU / len as f32;
        let (ws, wc) = angle.sin_cos();
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wc - ci * ws;
                ci = cr * ws + ci * wc;
                cr = next;
            }
        }
        len <<= 1;
    }
}

#[derive(Debug)]
pub struct Analyzer {
    rate: f32,
    decay: f32,
    source: usize,
    input: Vec<f32>,
    filled: usize,
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    levels: [f32; BANDS],
}

impl Default for Analyzer {
    fn default() -> Self {
        let window = (0..SIZE)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos())
            .collect();
        Self {
            rate: 48_000.0,
            decay: PARAMS[0].default,
            source: 0,
            input: vec![0.0; SIZE],
            filled: 0,
            window,
            re: vec![0.0; SIZE],
            im: vec![0.0; SIZE],
            levels: [-120.0; BANDS],
        }
    }
}

impl Analyzer {
    fn analyse(&mut self, elapsed: f32) {
        for i in 0..SIZE {
            self.re[i] = self.input[i] * self.window[i];
            self.im[i] = 0.0;
        }
        fft(&mut self.re, &mut self.im);
        let bin = self.rate / SIZE as f32;
        // Hann window coherent gain 0.5, one-sided spectrum.
        let norm = 4.0 / SIZE as f32;
        let half = SIZE / 2;
        for k in 0..half {
            self.re[k] = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt() * norm;
        }
        let magnitudes = &self.re[..half];
        let fall = self.decay * elapsed;
        for band in 0..BANDS {
            let lo = LOW * (HIGH / LOW).powf(band as f32 / BANDS as f32) / bin;
            let hi = LOW * (HIGH / LOW).powf((band + 1) as f32 / BANDS as f32) / bin;
            let (a, b) = (
                (lo.ceil() as usize).max(1),
                (hi.floor() as usize).min(half - 1),
            );
            let peak = if b > a {
                magnitudes[a..=b].iter().fold(0.0f32, |m, &v| m.max(v))
            } else {
                // Narrower than a bin: read between the two nearest.
                let at = (band_frequency(band) / bin).clamp(1.0, (half - 2) as f32);
                let (k, t) = (at as usize, at.fract());
                magnitudes[k] + (magnitudes[k + 1] - magnitudes[k]) * t
            };
            let db = crate::gain_to_db(peak).max(-120.0);
            self.levels[band] = db.max(self.levels[band] - fall);
        }
    }
}

impl Effect for Analyzer {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.filled = 0;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.decay = value,
            2 => self.source = (value.round().max(0.0) as usize).min(SOURCES.len() - 1),
            _ => {}
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter().zip(right.iter()) {
            self.input[self.filled] = match self.source {
                1 => *l,
                2 => *r,
                3 => (l - r) * 0.5,
                _ => (l + r) * 0.5,
            };
            self.filled += 1;
            if self.filled == SIZE {
                self.analyse(HOP as f32 / self.rate);
                self.input.copy_within(HOP.., 0);
                self.filled = SIZE - HOP;
            }
        }
    }

    fn reset(&mut self) {
        self.input.fill(0.0);
        self.filled = 0;
        self.levels = [-120.0; BANDS];
    }

    fn meters(&self, out: &mut [f32]) {
        let n = out.len().min(BANDS);
        out[..n].copy_from_slice(&self.levels[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The band levels after a second of a sine on each side.
    fn levels_of(analyzer: &mut Analyzer, (freq_l, freq_r): (f32, f32)) -> [f32; BANDS] {
        analyzer.prepare(48_000, 1024);
        for block in 0..47 {
            let sine = |f: f32| -> Vec<f32> {
                (0..1024)
                    .map(|i| {
                        let t = (block * 1024 + i) as f32 / 48_000.0;
                        0.5 * (std::f32::consts::TAU * f * t).sin()
                    })
                    .collect()
            };
            let (mut l, mut r) = (sine(freq_l), sine(freq_r));
            analyzer.process(&mut l, &mut r);
        }
        let mut levels = [0.0; BANDS];
        analyzer.meters(&mut levels);
        levels
    }

    fn loudest(levels: &[f32; BANDS]) -> f32 {
        let band = (0..BANDS)
            .max_by(|&a, &b| levels[a].total_cmp(&levels[b]))
            .unwrap();
        band_frequency(band)
    }

    #[test]
    fn the_source_picks_the_channels() {
        let mut analyzer = Analyzer::default();
        analyzer.set_param(2, 1.0);
        assert!(
            (loudest(&levels_of(&mut analyzer, (200.0, 5000.0))) / 200.0)
                .log2()
                .abs()
                < 0.1
        );
        analyzer.set_param(2, 2.0);
        analyzer.reset();
        assert!(
            (loudest(&levels_of(&mut analyzer, (200.0, 5000.0))) / 5000.0)
                .log2()
                .abs()
                < 0.1
        );
        // The same sine on both sides has no side.
        analyzer.set_param(2, 3.0);
        analyzer.reset();
        let side = levels_of(&mut analyzer, (1000.0, 1000.0));
        assert!(side.iter().all(|&db| db < -80.0));
    }

    #[test]
    fn low_bands_narrower_than_a_bin_still_read_a_bass() {
        let mut analyzer = Analyzer::default();
        let levels = levels_of(&mut analyzer, (40.0, 40.0));
        assert!((loudest(&levels) / 40.0).log2().abs() < 0.2);
        assert!(levels.iter().filter(|&&db| db > -30.0).count() > 1);
    }

    #[test]
    fn ranges_set_the_floor() {
        assert_eq!(range_floor(0.0), -60.0);
        assert_eq!(range_floor(1.0), -90.0);
        assert_eq!(range_floor(2.0), -120.0);
    }

    #[test]
    fn a_sine_lights_its_band() {
        let mut analyzer = Analyzer::default();
        analyzer.prepare(48_000, 1024);
        let freq = 1000.0;
        let mut n = 0usize;
        for _ in 0..20 {
            let mut l: Vec<f32> = (0..1024)
                .map(|i| 0.5 * (std::f32::consts::TAU * freq * (n + i) as f32 / 48_000.0).sin())
                .collect();
            let mut r = l.clone();
            n += 1024;
            analyzer.process(&mut l, &mut r);
            assert!(
                (l[10]
                    - 0.5
                        * (std::f32::consts::TAU * freq * (n - 1024 + 10) as f32 / 48_000.0).sin())
                .abs()
                    < 1e-6
            );
        }
        let mut levels = [0.0; BANDS];
        analyzer.meters(&mut levels);
        let loudest = (0..BANDS)
            .max_by(|&a, &b| levels[a].total_cmp(&levels[b]))
            .unwrap();
        assert!(
            (band_frequency(loudest) / freq).log2().abs() < 0.2,
            "band {loudest}"
        );
        assert!(
            (levels[loudest] + 6.02).abs() < 1.5,
            "{} dB",
            levels[loudest]
        );
        assert!(levels[2] < -60.0);
    }
}
