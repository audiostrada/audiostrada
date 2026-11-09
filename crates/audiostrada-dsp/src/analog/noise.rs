//! The noise floor of a circuit: a white hiss, a flicker (1/f) noise rising below its corner, and
//! the hum of the mains with its first harmonics.

use super::dbu;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseSettings {
    /// Level of the hiss over the whole band, dBu.
    pub hiss_dbu: f32,
    /// Below this frequency the flicker noise exceeds the hiss, Hz; 0 for none.
    pub flicker_hz: f32,
    /// The mains frequency (50 or 60 Hz) and the level of its hum, dBu; `None` for none.
    pub hum: Option<(f32, f32)>,
}

impl Default for NoiseSettings {
    /// A quiet console channel.
    fn default() -> Self {
        Self {
            hiss_dbu: -88.0,
            flicker_hz: 200.0,
            hum: None,
        }
    }
}

/// Levels of the hum's harmonics against its fundamental: a little buzz.
const HUM_HARMONICS: [f32; 3] = [1.0, 0.5, 0.25];

#[derive(Clone, Debug)]
pub struct Noise {
    state: u32,
    white: f32,
    pink: f32,
    b: [f32; 3],
    hum: f32,
    phase: f64,
    step: f64,
}

/// Paul Kellet's pink filter: three one-poles and a direct path.
const POLES: [(f32, f32); 3] = [(0.99765, 0.099046), (0.963, 0.2965164), (0.57, 1.0526913)];
const DIRECT: f32 = 0.1848;

impl Noise {
    pub fn new(settings: NoiseSettings, rate: f64, seed: u32) -> Self {
        // Uniform numbers from -1 to 1 have a mean square of 1/3.
        let white = dbu(settings.hiss_dbu) * 3f32.sqrt();
        // The pink part matches the hiss's density at the flicker corner.
        let pink = if settings.flicker_hz > 0.0 {
            let w = std::f64::consts::TAU * f64::from(settings.flicker_hz) / rate;
            let (cos, sin) = (w.cos(), -w.sin());
            let mut re = f64::from(DIRECT);
            let mut im = 0.0;
            for (pole, gain) in POLES {
                // gain / (1 - pole e^-jw)
                let (dr, di) = (1.0 - f64::from(pole) * cos, -f64::from(pole) * sin);
                let d = dr * dr + di * di;
                re += f64::from(gain) * dr / d;
                im -= f64::from(gain) * di / d;
            }
            white / (re * re + im * im).sqrt() as f32
        } else {
            0.0
        };
        let (mains, level) = settings.hum.unwrap_or((50.0, f32::NEG_INFINITY));
        Self {
            state: seed.max(1),
            white,
            pink,
            b: [0.0; 3],
            hum: if level.is_finite() {
                dbu(level) * std::f32::consts::SQRT_2
            } else {
                0.0
            },
            phase: 0.0,
            step: std::f64::consts::TAU * f64::from(mains) / rate,
        }
    }

    fn uniform(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    pub fn sample(&mut self) -> f32 {
        let mut out = self.white * self.uniform();
        if self.pink > 0.0 {
            let w = self.uniform();
            let mut pink = DIRECT * w;
            for (b, (pole, gain)) in self.b.iter_mut().zip(POLES) {
                *b = pole * *b + gain * w;
                pink += *b;
            }
            out += self.pink * pink;
        }
        if self.hum > 0.0 {
            self.phase = (self.phase + self.step) % std::f64::consts::TAU;
            for (k, level) in HUM_HARMONICS.iter().enumerate() {
                out += self.hum * level * ((k + 1) as f64 * self.phase).sin() as f32;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use realfft::RealFftPlanner;

    fn spectrum(noise: &mut Noise) -> Vec<f64> {
        let n = 48_000;
        let fft = RealFftPlanner::<f64>::new().plan_fft_forward(n);
        let mut x: Vec<f64> = (0..n).map(|_| f64::from(noise.sample())).collect();
        let mut out = fft.make_output_vec();
        let _ = fft.process(&mut x, &mut out);
        out.iter().map(|c| c.norm_sqr()).collect()
    }

    fn band(power: &[f64], lo: usize, hi: usize) -> f64 {
        10.0 * (power[lo..hi].iter().sum::<f64>() / (hi - lo) as f64).log10()
    }

    #[test]
    fn the_hiss_is_at_its_level() {
        let mut noise = Noise::new(
            NoiseSettings {
                hiss_dbu: -80.0,
                flicker_hz: 0.0,
                hum: None,
            },
            48_000.0,
            3,
        );
        let x: Vec<f32> = (0..48_000).map(|_| noise.sample()).collect();
        let rms = (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let level = 20.0 * (rms / super::super::WORKING).log10();
        assert!((level + 80.0).abs() < 0.5, "{level}");
    }

    #[test]
    fn flicker_rises_in_the_lows_and_hum_shows_its_lines() {
        let mut noise = Noise::new(
            NoiseSettings {
                hiss_dbu: -80.0,
                flicker_hz: 300.0,
                hum: Some((50.0, -70.0)),
            },
            48_000.0,
            9,
        );
        let power = spectrum(&mut noise);
        // 1/f: about 10 dB more density two decades under the corner than over it.
        let (low, high) = (band(&power, 20, 40), band(&power, 4_000, 8_000));
        assert!(low > high + 8.0, "{low} {high}");
        // The mains, then a quieter 2nd harmonic, far above the floor around.
        let at = |f: usize| 10.0 * power[f].log10();
        assert!(at(50) > at(100) + 4.0 && at(100) > band(&power, 101, 140) + 20.0);
        let mut same = Noise::new(NoiseSettings::default(), 48_000.0, 4);
        let mut again = Noise::new(NoiseSettings::default(), 48_000.0, 4);
        assert!((0..100).all(|_| same.sample() == again.sample()));
    }
}
