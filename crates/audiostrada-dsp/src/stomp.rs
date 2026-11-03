//! Drive pedals voiced after the most played circuits (clipping path filters, diodes or
//! transistors, tone control), run four times oversampled, and a wah.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::drive::{FACTOR, Oversampler, Taps};
use crate::{db_to_gain, time_coefficient};

/// How a circuit clips.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Clip {
    /// Diodes in the op-amp feedback: soft and symmetric.
    Soft,
    /// Unequal diodes in the feedback: soft and asymmetric.
    SoftAsym,
    /// Diodes to ground: hard.
    Hard,
    /// Germanium diodes or transistor: low threshold, asymmetric.
    Germanium,
    /// Two hard transistor stages in a row.
    Cascade,
}

fn clip(kind: Clip, x: f32) -> f32 {
    let hard = |x: f32, k: f32| x / (1.0 + x.abs().powf(k)).powf(1.0 / k);
    match kind {
        Clip::Soft => x.tanh(),
        Clip::SoftAsym => {
            if x >= 0.0 {
                x.tanh()
            } else {
                (1.5 * x).tanh() / 1.5
            }
        }
        Clip::Hard => hard(x, 2.5),
        Clip::Germanium => (x + 0.25).tanh() - 0.25f32.tanh(),
        Clip::Cascade => hard(hard(x, 2.0) * 4.0, 3.0),
    }
}

/// Tone control after the clipping.
#[derive(Clone, Copy, Debug)]
enum Tone {
    /// Low-pass sweeping from the first frequency (knob at 0) to the second.
    Sweep(f64, f64),
    /// Treble shelf at this frequency, up to the given dB either way.
    Tilt(f64, f64),
    /// Blend of a low-pass and a high-pass, scooping the mids in between.
    Scoop(f64, f64),
}

/// A drive pedal's circuit.
#[derive(Clone, Copy, Debug)]
pub struct Circuit {
    /// High-pass in front of the clipping (the clean signal skips it when `clean` > 0).
    hp: f64,
    /// Peak before the clipping (Hz, dB), for mid-focused circuits.
    focus: (f64, f64),
    /// Gain range of the drive knob (dB).
    gain: (f32, f32),
    clip: Clip,
    /// Clean signal added to the clipped one: the overdrive's mid hump, a boost's blend.
    clean: f32,
    /// Op-amp bandwidth at full gain (Hz).
    bandwidth: f32,
    tone: Tone,
}

const fn circuit(
    hp: f64,
    focus: (f64, f64),
    gain: (f32, f32),
    clip: Clip,
    clean: f32,
    bandwidth: f32,
    tone: Tone,
) -> Circuit {
    Circuit {
        hp,
        focus,
        gain,
        clip,
        clean,
        bandwidth,
        tone,
    }
}

/// Mid-hump overdrive: the clipping path is high-passed at 720 Hz (Ibanez TS808/TS9).
pub const GREEN: Circuit = circuit(
    720.0,
    (0.0, 0.0),
    (12.0, 41.0),
    Clip::Soft,
    2.0,
    6000.0,
    Tone::Sweep(500.0, 3500.0),
);
/// Asymmetric overdrive (Boss SD-1).
pub const YELLOW: Circuit = circuit(
    720.0,
    (0.0, 0.0),
    (14.0, 44.0),
    Clip::SoftAsym,
    1.0,
    20_000.0,
    Tone::Sweep(800.0, 7000.0),
);
/// Transparent overdrive: clean and germanium-clipped paths blended (Klon Centaur).
pub const GOLD: Circuit = circuit(
    300.0,
    (1000.0, 2.0),
    (6.0, 36.0),
    Clip::Germanium,
    0.8,
    20_000.0,
    Tone::Tilt(1500.0, 9.0),
);
/// Wide, open overdrive with lots of range (Boss BD-2).
pub const BLUE: Circuit = circuit(
    100.0,
    (0.0, 0.0),
    (10.0, 42.0),
    Clip::SoftAsym,
    0.0,
    20_000.0,
    Tone::Tilt(1800.0, 10.0),
);
/// Hard-clipping distortion with a scooped tilt (Boss DS-1).
pub const ORANGE: Circuit = circuit(
    90.0,
    (500.0, -3.0),
    (18.0, 46.0),
    Clip::Hard,
    0.0,
    20_000.0,
    Tone::Scoop(700.0, 1800.0),
);
/// Op-amp distortion: the gain eats the bandwidth, the filter cuts from the top
/// (ProCo RAT).
pub const BLACK: Circuit = circuit(
    60.0,
    (0.0, 0.0),
    (12.0, 60.0),
    Clip::Hard,
    0.0,
    3500.0,
    Tone::Sweep(475.0, 16_000.0),
);
/// Two-stage high-gain distortion with scooped mids (Boss MT-2).
pub const METAL: Circuit = circuit(
    150.0,
    (900.0, 6.0),
    (20.0, 46.0),
    Clip::Cascade,
    0.0,
    20_000.0,
    Tone::Scoop(400.0, 2200.0),
);
/// Germanium fuzz: round, spitty, cleans up with the guitar (Dallas Fuzz Face).
pub const VINTAGE_FUZZ: Circuit = circuit(
    30.0,
    (0.0, 0.0),
    (16.0, 46.0),
    Clip::Germanium,
    0.0,
    20_000.0,
    Tone::Tilt(2500.0, 4.0),
);
/// Violin-like sustain with the scooped tone stack (Electro-Harmonix Big Muff Pi).
pub const BIG_FUZZ: Circuit = circuit(
    100.0,
    (0.0, 0.0),
    (14.0, 40.0),
    Clip::Cascade,
    0.0,
    20_000.0,
    Tone::Scoop(500.0, 1200.0),
);
/// Germanium treble booster into the amp (Dallas Rangemaster).
pub const TREBLE: Circuit = circuit(
    1200.0,
    (0.0, 0.0),
    (6.0, 26.0),
    Clip::Germanium,
    0.15,
    20_000.0,
    Tone::Tilt(3000.0, 6.0),
);

/// A drive pedal running one of the circuits; knobs drive, tone and level from 0 to 1.
#[derive(Debug)]
pub struct Stomp {
    stage: Oversampler,
    pre: [(Coefficients, Biquad); 2],
    band: (Coefficients, Biquad),
    tone: [(Coefficients, Biquad); 2],
    scoop: f32,
    scooped: bool,
    gain: f32,
    clean: f32,
    clip: Clip,
    out: f32,
}

impl Default for Stomp {
    fn default() -> Self {
        let filter = || (Coefficients::IDENTITY, Biquad::default());
        Self {
            stage: Oversampler::new(Taps::Short),
            pre: [filter(), filter()],
            band: filter(),
            tone: [filter(), filter()],
            scoop: 0.5,
            scooped: false,
            gain: 1.0,
            clean: 0.0,
            clip: Clip::Soft,
            out: 1.0,
        }
    }
}

impl Stomp {
    pub fn set(&mut self, c: &Circuit, drive: f32, tone: f32, level: f32, rate: f32) {
        let os = f64::from(rate) * FACTOR as f64;
        let rate = f64::from(rate);
        let design = |kind, f: f64, q, g: f64, fs: f64| Coefficients::design(kind, f, q, g, fs);
        let db = c.gain.0 + (c.gain.1 - c.gain.0) * drive;
        self.gain = db_to_gain(db);
        self.pre[0].0 = design(FilterKind::HighPass, c.hp, 0.707, 0.0, os);
        self.pre[1].0 = if c.focus.1 == 0.0 {
            Coefficients::IDENTITY
        } else {
            design(FilterKind::Peak, c.focus.0, 0.8, c.focus.1, os)
        };
        // More gain, less op-amp bandwidth.
        let bandwidth = 20_000.0 - (20_000.0 - c.bandwidth) * drive;
        self.band.0 = design(FilterKind::LowPass, f64::from(bandwidth), 0.707, 0.0, os);
        self.clip = c.clip;
        self.clean = c.clean;
        self.scooped = false;
        let t = f64::from(tone);
        match c.tone {
            Tone::Sweep(low, high) => {
                let f = low * (high / low).powf(t);
                self.tone[0].0 = design(FilterKind::LowPass, f, 0.707, 0.0, rate);
                self.tone[1].0 = Coefficients::IDENTITY;
            }
            Tone::Tilt(f, range) => {
                self.tone[0].0 =
                    design(FilterKind::HighShelf, f, 0.7, (t - 0.5) * 2.0 * range, rate);
                self.tone[1].0 = design(FilterKind::LowPass, 9000.0, 0.707, 0.0, rate);
            }
            Tone::Scoop(low, high) => {
                self.tone[0].0 = design(FilterKind::LowPass, low, 0.707, 0.0, rate);
                self.tone[1].0 = design(FilterKind::HighPass, high, 0.707, 0.0, rate);
                self.scoop = tone;
                self.scooped = true;
            }
        }
        // The clipped part peaks near 1 whatever the drive; level 0.5 is about unity.
        let peak = (clip(c.clip, 0.3 * self.gain).abs() + 0.3 * c.clean).max(0.05);
        self.out = 0.4 / peak * db_to_gain((level - 0.5) * 30.0);
    }

    pub fn process(&mut self, buffer: &mut [f32]) {
        let (gain, clean, kind) = (self.gain, self.clean, self.clip);
        let (pre, band) = (&mut self.pre, &mut self.band);
        for s in buffer.iter_mut() {
            *s = self.stage.run(*s, |x| {
                let mut v = f64::from(x);
                for (c, f) in pre.iter_mut() {
                    v = f.process(c, v);
                }
                let wet = clip(kind, v as f32 * gain);
                let wet = band.1.process(&band.0, f64::from(wet)) as f32;
                wet + x * clean
            });
        }
        for s in buffer.iter_mut() {
            let v = f64::from(*s);
            let y = if self.scooped {
                let low = self.tone[0].1.process(&self.tone[0].0, v);
                let high = self.tone[1].1.process(&self.tone[1].0, v);
                low * f64::from(1.0 - self.scoop) + high * f64::from(self.scoop)
            } else {
                let y = self.tone[0].1.process(&self.tone[0].0, v);
                self.tone[1].1.process(&self.tone[1].0, y)
            };
            *s = y as f32 * self.out;
        }
    }

    pub fn reset(&mut self) {
        self.stage.clear();
        for (_, f) in self.pre.iter_mut().chain(self.tone.iter_mut()) {
            *f = Biquad::default();
        }
        self.band.1 = Biquad::default();
    }
}

/// Wah: a resonant band-pass swept by the pedal position or by the playing.
#[derive(Debug)]
pub struct Wah {
    position: f32,
    auto: f32,
    q: f32,
    env: f32,
    attack: f32,
    release: f32,
    smooth: f32,
    sweep: f32,
    s1: f32,
    s2: f32,
    rate: f32,
}

impl Default for Wah {
    fn default() -> Self {
        Self {
            position: 0.5,
            auto: 0.0,
            q: 4.0,
            env: 0.0,
            attack: 0.0,
            release: 0.0,
            smooth: 0.0,
            sweep: 0.5,
            s1: 0.0,
            s2: 0.0,
            rate: 48_000.0,
        }
    }
}

impl Wah {
    /// Pedal position, auto (envelope) amount and resonance, from 0 to 1.
    pub fn set(&mut self, position: f32, auto: f32, resonance: f32, rate: f32) {
        self.position = position;
        self.auto = auto;
        self.q = 2.0 + resonance * 8.0;
        self.rate = rate;
        self.attack = time_coefficient(4.0, rate);
        self.release = time_coefficient(120.0, rate);
        self.smooth = time_coefficient(10.0, rate);
    }

    pub fn process(&mut self, buffer: &mut [f32]) {
        for s in buffer.iter_mut() {
            let level = s.abs();
            let c = if level > self.env {
                self.attack
            } else {
                self.release
            };
            self.env = level + c * (self.env - level);
            let target = (self.position + self.auto * (self.env * 6.0).min(1.0)).min(1.0);
            self.sweep = target + self.smooth * (self.sweep - target);
            let freq = 350.0 * (2200.0f32 / 350.0).powf(self.sweep);
            // State-variable filter (topology preserving).
            let g = (std::f32::consts::PI * freq / self.rate).tan();
            let k = 1.0 / self.q;
            let h = 1.0 / (1.0 + g * (g + k));
            let high = (*s - (k + g) * self.s1 - self.s2) * h;
            let band = g * high + self.s1;
            self.s1 = g * high + band;
            let low = g * band + self.s2;
            self.s2 = g * band + low;
            *s = band * (1.0 + self.q * 0.4) + 0.1 * low;
        }
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
        self.env = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(c: &Circuit, drive: f32) -> Vec<f32> {
        let mut pedal = Stomp::default();
        pedal.set(c, drive, 0.5, 0.5, 48_000.0);
        let mut buffer: Vec<f32> = (0..24_000)
            .map(|i| {
                let t = i as f32 * std::f32::consts::TAU / 48_000.0;
                0.15 * (t * 220.0).sin() + 0.1 * (t * 880.0).sin() + 0.05 * (t * 2640.0).sin()
            })
            .collect();
        pedal.process(&mut buffer);
        buffer
    }

    #[test]
    fn every_circuit_is_bounded_and_near_unity() {
        for (name, c) in [
            ("green", GREEN),
            ("yellow", YELLOW),
            ("gold", GOLD),
            ("blue", BLUE),
            ("orange", ORANGE),
            ("black", BLACK),
            ("metal", METAL),
            ("vintage fuzz", VINTAGE_FUZZ),
            ("big fuzz", BIG_FUZZ),
            ("treble", TREBLE),
        ] {
            for drive in [0.0, 0.5, 1.0] {
                let out = run(&c, drive);
                let tail = &out[12_000..];
                let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
                assert!(out.iter().all(|s| s.is_finite() && s.abs() < 3.0), "{name}");
                assert!(rms > 0.02 && rms < 1.0, "{name} at {drive}: {rms}");
            }
        }
    }

    #[test]
    #[ignore = "prints the tonal balance of every circuit"]
    fn print_balance() {
        use realfft::RealFftPlanner;
        // A power chord with guitar-like harmonics.
        let chord: Vec<f32> = (0..48_000)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                (1..40)
                    .map(|n| {
                        let n = n as f32;
                        [110.0, 165.0, 220.0]
                            .iter()
                            .map(|f| (t * f * n * std::f32::consts::TAU).sin())
                            .sum::<f32>()
                            * 0.06
                            / n
                            * (-n / 12.0).exp()
                    })
                    .sum::<f32>()
            })
            .collect();
        let bands = [0.0, 250.0, 1000.0, 3000.0, 6000.0, 24_000.0];
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(32_768);
        for (name, c) in [
            ("green", GREEN),
            ("yellow", YELLOW),
            ("gold", GOLD),
            ("blue", BLUE),
            ("orange", ORANGE),
            ("black", BLACK),
        ] {
            for drive in [0.3, 0.7] {
                let mut pedal = Stomp::default();
                pedal.set(&c, drive, 0.5, 0.5, 48_000.0);
                let mut buffer = chord.clone();
                pedal.process(&mut buffer);
                let mut frame = buffer[8_000..8_000 + 32_768].to_vec();
                let mut spectrum = fft.make_output_vec();
                fft.process(&mut frame, &mut spectrum).unwrap();
                let energy: Vec<f32> = bands
                    .windows(2)
                    .map(|b| {
                        let bin = |f: f32| (f / 48_000.0 * 32_768.0) as usize;
                        spectrum[bin(b[0])..bin(b[1]).min(spectrum.len())]
                            .iter()
                            .map(|c| c.norm_sqr())
                            .sum()
                    })
                    .collect();
                let total: f32 = energy.iter().sum();
                let db: Vec<String> = energy
                    .iter()
                    .map(|e| format!("{:6.1}", 10.0 * (e / total).log10()))
                    .collect();
                println!("{name:7} drive {drive}: {}", db.join(" "));
            }
        }
    }

    #[test]
    fn wah_sweeps_its_peak() {
        let tone = |position: f32, f: f32| {
            let mut wah = Wah::default();
            wah.set(position, 0.0, 0.5, 48_000.0);
            let mut buffer: Vec<f32> = (0..24_000)
                .map(|i| 0.3 * (i as f32 * f * std::f32::consts::TAU / 48_000.0).sin())
                .collect();
            wah.process(&mut buffer);
            buffer[12_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()))
        };
        assert!(tone(0.0, 400.0) > tone(1.0, 400.0) * 2.0);
        assert!(tone(1.0, 2000.0) > tone(0.0, 2000.0) * 2.0);
    }
}
