//! Room reverb engine: early reflections, then an 8-line feedback delay network mixed by a
//! Hadamard matrix, with slowly modulated lines for a smooth tail and separate decay times for
//! the lows, mids and highs. Room, Hall and Space (the Reverb's Space and Shimmer) scale it.

use crate::line::{AllPass, Line};
use crate::{Effect, ParamInfo, choice, lowpass_coefficient, param, pole, switch};

const TYPES: [&str; 3] = ["Room", "Hall", "Space"];

const PARAMS: [ParamInfo; 14] = [
    choice("type", "Space", &TYPES, 1.0),
    param("size", "Size", 0.0, 100.0, 60.0, "%"),
    param("decay", "Decay", 0.2, 20.0, 2.4, "s"),
    param("predelay", "Pre-delay", 0.0, 200.0, 20.0, "ms"),
    param("early", "Early / late", 0.0, 100.0, 40.0, "%"),
    param("treble", "Treble decay", 10.0, 100.0, 45.0, "%"),
    param("bass", "Bass decay", 50.0, 200.0, 120.0, "%"),
    param("diffusion", "Diffusion", 0.0, 100.0, 80.0, "%"),
    param("modulation", "Modulation", 0.0, 100.0, 40.0, "%"),
    param("low_cut", "Low cut", 20.0, 1000.0, 100.0, "Hz"),
    param("high_cut", "High cut", 1000.0, 20000.0, 9000.0, "Hz"),
    param("width", "Width", 0.0, 150.0, 100.0, "%"),
    param("mix", "Mix", 0.0, 100.0, 25.0, "%"),
    switch("freeze", "Freeze", 0.0),
];

const LINES: usize = 8;
/// Line lengths at full size of a hall, in milliseconds (mutually prime in samples).
const LENGTHS_MS: [f32; LINES] = [31.3, 37.9, 41.3, 47.9, 53.1, 61.7, 67.3, 79.9];
const TYPE_SCALE: [f32; 3] = [0.35, 1.0, 1.6];
/// Early reflection taps: time (fraction of the room time), gain, pan (-1 left to 1 right).
const EARLY: [(f32, f32, f32); 12] = [
    (0.043, 0.84, -0.6),
    (0.077, 0.75, 0.7),
    (0.112, 0.68, -0.2),
    (0.159, 0.62, 0.4),
    (0.197, 0.55, -0.9),
    (0.243, 0.50, 0.9),
    (0.302, 0.44, -0.4),
    (0.361, 0.40, 0.2),
    (0.420, 0.34, -0.7),
    (0.503, 0.30, 0.6),
    (0.611, 0.25, -0.1),
    (0.733, 0.20, 0.3),
];
const MAX_MOD_MS: f32 = 1.2;
const MAX_PREDELAY_MS: f32 = 200.0;
const CROSSOVER_HZ: f32 = 250.0;

/// Room time of the early reflections in seconds for a type and size.
fn room_time(kind: usize, size: f32) -> f32 {
    0.08 * TYPE_SCALE[kind.min(2)] * (0.5 + size / 100.0)
}

/// In-place fast Walsh-Hadamard transform of 8 values, normalized (energy preserving).
#[inline]
fn hadamard(v: &mut [f32; LINES]) {
    let mut h = 1;
    while h < LINES {
        for i in (0..LINES).step_by(h * 2) {
            for j in i..i + h {
                let (a, b) = (v[j], v[j + h]);
                v[j] = a + b;
                v[j + h] = a - b;
            }
        }
        h *= 2;
    }
    let norm = 1.0 / (LINES as f32).sqrt();
    for x in v.iter_mut() {
        *x *= norm;
    }
}

#[derive(Debug)]
pub(crate) struct Hall {
    rate: f32,
    values: [f32; 14],
    predelay: Line,
    early: Line,
    diffusers: [AllPass; 4],
    lines: [Line; LINES],
    lengths: [f32; LINES],
    /// Per line: gains of the lows and of the rest, and the crossover and damping states.
    low_gain: [f32; LINES],
    high_gain: [f32; LINES],
    split: [f32; LINES],
    damp: [f32; LINES],
    damp_state: [f32; LINES],
    phases: [f32; LINES],
    hp: [(f32, f32); 2],
    lp: [f32; 2],
}

impl Default for Hall {
    fn default() -> Self {
        let mut hall = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            predelay: Line::default(),
            early: Line::default(),
            diffusers: Default::default(),
            lines: Default::default(),
            lengths: [0.0; LINES],
            low_gain: [0.0; LINES],
            high_gain: [0.0; LINES],
            split: [0.0; LINES],
            damp: [0.0; LINES],
            damp_state: [0.0; LINES],
            phases: std::array::from_fn(|i| i as f32 * 0.785),
            hp: [(0.0, 0.0); 2],
            lp: [0.0; 2],
        };
        hall.allocate();
        hall
    }
}

/// Approximate decay of the lows, mids and highs in seconds.
pub(crate) fn decay_seconds(decay: f32, bass: f32, treble: f32) -> (f32, f32, f32) {
    (decay * bass / 100.0, decay, decay * treble / 100.0)
}

impl Hall {
    fn allocate(&mut self) {
        let ms = |ms: f32| (ms * 0.001 * self.rate) as usize;
        let max_scale = TYPE_SCALE[2] * 1.5;
        self.predelay = Line::new(ms(MAX_PREDELAY_MS) + 4);
        let early = ms(room_time(2, 100.0) * 1000.0) + 8;
        self.early = Line::new(early);
        let diffuser_ms = [4.7, 3.6, 12.7, 9.3];
        self.diffusers = diffuser_ms.map(|d| AllPass {
            line: Line::new(ms(d * max_scale) + 8),
            length: 0.0,
        });
        self.lines = LENGTHS_MS.map(|l| Line::new(ms(l * max_scale + MAX_MOD_MS) + 8));
        self.update();
        self.reset();
    }

    // Parallel per-line arrays read clearer with an index.
    #[allow(clippy::needless_range_loop)]
    fn update(&mut self) {
        let kind = self.values[0].round().clamp(0.0, 2.0) as usize;
        let scale = TYPE_SCALE[kind] * (0.5 + self.values[1] / 100.0);
        let rate = self.rate;
        let diffuser_ms = [4.7, 3.6, 12.7, 9.3];
        for (a, d) in self.diffusers.iter_mut().zip(diffuser_ms) {
            a.length = d * scale * 0.001 * rate;
        }
        let (low, mid, high) = decay_seconds(self.values[2], self.values[6], self.values[5]);
        for i in 0..LINES {
            let length = LENGTHS_MS[i] * scale * 0.001 * rate;
            self.lengths[i] = length;
            let seconds = length / rate;
            let gain = |rt: f32| 10f32.powf(-3.0 * seconds / rt.max(0.05));
            self.low_gain[i] = gain(low);
            // Mids keep the decay time; the damping filter takes the highs further down.
            self.high_gain[i] = gain(mid);
            let treble_loss = gain(high) / gain(mid);
            // One-pole low-pass whose loss at 8 kHz matches the treble decay.
            let w = std::f32::consts::TAU * 8000.0_f32.min(rate * 0.45) / rate;
            let target = treble_loss.clamp(0.05, 1.0);
            // |H(w)| of y += c (x - y): solve for c by bisection.
            let response =
                |c: f32| c / (1.0 - 2.0 * (1.0 - c) * w.cos() + (1.0 - c).powi(2)).sqrt();
            let (mut lo, mut hi) = (0.001f32, 1.0f32);
            for _ in 0..24 {
                let m = 0.5 * (lo + hi);
                if response(m) < target {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            self.damp[i] = hi;
        }
    }
}

impl Effect for Hall {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.update();
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [
            kind,
            size,
            _,
            predelay,
            early_mix,
            _,
            _,
            diffusion,
            modulation,
            low_cut,
            high_cut,
            width,
            mix,
            freeze,
        ] = self.values;
        let frozen = freeze >= 0.5;
        let kind = kind.round().clamp(0.0, 2.0) as usize;
        let rate = self.rate;
        let pre = (predelay * 0.001 * rate).max(1.0);
        let room = room_time(kind, size) * rate;
        let g_diff = 0.7 * diffusion / 100.0;
        let excursion = modulation / 100.0 * MAX_MOD_MS * 0.001 * rate;
        let lfo_step = std::f32::consts::TAU * 0.37 / rate;
        let split_c = lowpass_coefficient(CROSSOVER_HZ, rate);
        let hp_c = pole(low_cut, rate);
        let lp_c = lowpass_coefficient(high_cut, rate);
        let (early_gain, late_gain) = (early_mix / 100.0, 1.0 - early_mix / 100.0 * 0.6);
        let (wet, dry) = (mix / 100.0, 1.0 - mix / 100.0);
        let w = width / 100.0;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = if frozen { 0.0 } else { 0.5 * (*l + *r) };
            self.predelay.push(input);
            let x = self.predelay.read(pre);
            // Early reflections from a stereo tapped delay.
            self.early.push(x);
            let mut er = [0.0f32; 2];
            for &(t, g, pan) in &EARLY {
                let tap = self.early.read(t * room);
                er[0] += tap * g * (1.0 - pan) * 0.5;
                er[1] += tap * g * (1.0 + pan) * 0.5;
            }
            // Diffused input into the network.
            let mut d = x;
            for a in &mut self.diffusers {
                d = a.process(d, g_diff, 0.0);
            }
            let mut outs = [0.0f32; LINES];
            for i in 0..LINES {
                self.phases[i] =
                    (self.phases[i] + lfo_step * (1.0 + i as f32 * 0.13)) % std::f32::consts::TAU;
                let delay = self.lengths[i] + excursion * (1.0 + self.phases[i].sin());
                let v = self.lines[i].read(delay);
                // Two-band decay: lows and the rest get their own gain, then the highs fade.
                self.split[i] += split_c * (v - self.split[i]);
                let low = self.split[i];
                outs[i] = if frozen {
                    v
                } else {
                    low * self.low_gain[i] + (v - low) * self.high_gain[i]
                };
            }
            let mut mixed = outs;
            hadamard(&mut mixed);
            for i in 0..LINES {
                let damp = if frozen { 1.0 } else { self.damp[i] };
                self.damp_state[i] += damp * (mixed[i] - self.damp_state[i]);
                let damped = self.damp_state[i];
                let feed = damped + if i % 2 == 0 { d } else { -d } * 0.5;
                self.lines[i].push(feed);
            }
            let mut late = [0.0f32; 2];
            for (i, o) in outs.iter().enumerate() {
                let sign = if (i / 2) % 2 == 0 { 1.0 } else { -1.0 };
                if i % 2 == 0 {
                    late[0] += o;
                    late[1] += o * sign;
                } else {
                    late[1] += o;
                    late[0] -= o * sign;
                }
            }
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                out[ch] = er[ch] * early_gain + late[ch] * 0.35 * late_gain;
            }
            let mid = 0.5 * (out[0] + out[1]);
            let side = 0.5 * (out[0] - out[1]) * w;
            let mut out = [mid + side, mid - side];
            for (ch, s) in out.iter_mut().enumerate() {
                let (x1, y1) = self.hp[ch];
                let y = hp_c * (y1 + *s - x1);
                self.hp[ch] = (*s, y);
                self.lp[ch] += lp_c * (y - self.lp[ch]);
                *s = self.lp[ch];
            }
            *l = *l * dry + out[0] * wet;
            *r = *r * dry + out[1] * wet;
        }
    }

    fn reset(&mut self) {
        self.predelay.clear();
        self.early.clear();
        for a in &mut self.diffusers {
            a.line.clear();
        }
        for l in &mut self.lines {
            l.clear();
        }
        self.split = [0.0; LINES];
        self.damp_state = [0.0; LINES];
        self.hp = [(0.0, 0.0); 2];
        self.lp = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(hall: &mut Hall, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (48_000.0 * seconds) as usize;
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        l[0] = 1.0;
        r[0] = 1.0;
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            hall.process(cl, cr);
        }
        (l, r)
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|s| s * s).sum()
    }

    /// Time for the energy of `x` to fall 30 dB from its peak, in seconds (T30 x 2 = RT60).
    fn rt60(x: &[f32]) -> f32 {
        let mut tail: Vec<f32> = x
            .iter()
            .rev()
            .scan(0.0, |acc, s| {
                *acc += s * s;
                Some(*acc)
            })
            .collect();
        tail.reverse();
        let start = tail[0];
        let a = tail
            .iter()
            .position(|&e| e < start * 10f32.powf(-0.5))
            .unwrap_or(0);
        let b = tail
            .iter()
            .position(|&e| e < start * 10f32.powf(-3.5))
            .unwrap_or(x.len());
        2.0 * (b - a) as f32 / 48_000.0
    }

    #[test]
    fn decay_time_follows_the_setting() {
        for decay in [1.0, 3.0] {
            let mut hall = Hall::default();
            hall.set_param(2, decay);
            hall.set_param(12, 100.0);
            hall.set_param(4, 0.0);
            hall.prepare(48_000, 256);
            let (l, r) = impulse(&mut hall, decay * 2.5);
            assert!(l.iter().chain(&r).all(|s| s.is_finite()));
            let measured = rt60(&l);
            assert!(
                (measured - decay).abs() < decay * 0.35,
                "decay {decay} s measured {measured} s"
            );
        }
    }

    #[test]
    fn every_space_is_stable_and_stereo() {
        for kind in 0..TYPES.len() {
            let mut hall = Hall::default();
            hall.set_param(0, kind as f32);
            hall.set_param(2, 20.0);
            hall.set_param(6, 200.0);
            hall.set_param(12, 100.0);
            hall.prepare(48_000, 256);
            let (l, r) = impulse(&mut hall, 3.0);
            assert!(
                l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 4.0),
                "{kind}"
            );
            assert!(energy(&l[96_000..]) > 1e-4, "long decay keeps ringing");
            let different: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
            assert!(different > 1.0, "{kind}");
        }
    }
}
