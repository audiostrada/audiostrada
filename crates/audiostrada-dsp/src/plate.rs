//! Plate reverb after Jon Dattorro ("Effect Design, Part 1", 1997): input diffusers feed a
//! figure-eight tank of modulated all-passes and delays, read at many taps for a dense,
//! bright and smooth tail. Delay lengths are scaled from the paper's 29761 Hz.

use crate::line::{AllPass, Line};
use crate::{Effect, ParamInfo, lowpass_coefficient, param, pole, switch};

const PARAMS: [ParamInfo; 10] = [
    param("predelay", "Pre-delay", 0.0, 200.0, 12.0, "ms"),
    param("decay", "Decay", 0.0, 100.0, 55.0, "%"),
    param("damping", "Damping", 1000.0, 20000.0, 7000.0, "Hz"),
    param("bandwidth", "Input tone", 1000.0, 20000.0, 12000.0, "Hz"),
    param("diffusion", "Diffusion", 0.0, 100.0, 75.0, "%"),
    param("modulation", "Modulation", 0.0, 100.0, 35.0, "%"),
    param("low_cut", "Low cut", 20.0, 1000.0, 150.0, "Hz"),
    param("width", "Width", 0.0, 150.0, 100.0, "%"),
    param("mix", "Mix", 0.0, 100.0, 25.0, "%"),
    switch("freeze", "Freeze", 0.0),
];

const PAPER_RATE: f32 = 29_761.0;
/// Two input all-passes more than the paper (lengths at its rate): the tank gets a denser
/// burst, and the tail is diffuse within 30 ms instead of 200, as on a real plate (and as
/// Valhalla's).
const EXTRA: [f32; 2] = [331.0, 241.0];
const MAX_PREDELAY_MS: f32 = 200.0;

/// Most the tank keeps on each pass.
const MAX_DECAY_GAIN: f32 = 0.98;
/// One pass through half the tank: 672 + 4453 + 1800 + 3720 samples at the paper's rate.
const HALF_PASS: f32 = 10_645.0 / PAPER_RATE;

/// Decay coefficient of the tank for the decay parameter (0 to 100 %).
fn decay_gain(percent: f32) -> f32 {
    (percent / 100.0).clamp(0.0, 1.0) * MAX_DECAY_GAIN
}

/// The decay parameter giving a decay to -60 dB of `seconds` (before damping).
pub(crate) fn decay_percent(seconds: f32) -> f32 {
    let g = 10f32.powf(-3.0 * HALF_PASS / seconds.max(0.01));
    (g / MAX_DECAY_GAIN * 100.0).clamp(0.0, 100.0)
}

/// Approximate decay to -60 dB in seconds for the decay parameter and damping frequency.
pub(crate) fn decay_seconds(percent: f32, damping: f32) -> (f32, f32) {
    let g = decay_gain(percent);
    let rt = |g: f32| 3.0 * HALF_PASS / -g.max(1e-4).log10();
    // Highs also lose the damping filter's attenuation at 8 kHz on every pass.
    let at_8k = (damping / 8000.0).min(1.0).powf(0.7);
    (rt(g), rt(g * at_8k))
}

#[derive(Debug)]
pub(crate) struct Plate {
    rate: f32,
    values: [f32; 10],
    predelay: Line,
    input_lp: f32,
    diffusers: [AllPass; 6],
    /// Tank halves: modulated all-pass, delay, damping state, all-pass, delay.
    mod_ap: [AllPass; 2],
    delay_a: [Line; 2],
    damp_state: [f32; 2],
    ap2: [AllPass; 2],
    delay_b: [Line; 2],
    /// Paper delay lengths scaled to the rate, for the output taps.
    scale: f32,
    lfo: f32,
    excursion: f32,
    hp: [f32; 2],
    hp_in: [f32; 2],
}

impl Default for Plate {
    fn default() -> Self {
        let mut plate = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            predelay: Line::default(),
            input_lp: 0.0,
            diffusers: Default::default(),
            mod_ap: Default::default(),
            delay_a: Default::default(),
            damp_state: [0.0; 2],
            ap2: Default::default(),
            delay_b: Default::default(),
            scale: 1.0,
            lfo: 0.0,
            excursion: 0.0,
            hp: [0.0; 2],
            hp_in: [0.0; 2],
        };
        plate.allocate();
        plate
    }
}

impl Plate {
    fn allocate(&mut self) {
        let s = self.rate / PAPER_RATE;
        self.scale = s;
        self.excursion = 16.0 * s;
        let n = |len: f32| len * s;
        self.predelay = Line::new((MAX_PREDELAY_MS * 0.001 * self.rate) as usize + 4);
        self.diffusers = [
            AllPass::new(n(142.0), 0.0),
            AllPass::new(n(107.0), 0.0),
            AllPass::new(n(379.0), 0.0),
            AllPass::new(n(277.0), 0.0),
            AllPass::new(n(EXTRA[0]), 0.0),
            AllPass::new(n(EXTRA[1]), 0.0),
        ];
        self.mod_ap = [
            AllPass::new(n(672.0), self.excursion),
            AllPass::new(n(908.0), self.excursion),
        ];
        self.delay_a = [
            Line::new(n(4453.0) as usize + 2),
            Line::new(n(4217.0) as usize + 2),
        ];
        self.ap2 = [AllPass::new(n(1800.0), 0.0), AllPass::new(n(2656.0), 0.0)];
        self.delay_b = [
            Line::new(n(3720.0) as usize + 2),
            Line::new(n(3163.0) as usize + 2),
        ];
        self.reset();
    }

    #[inline]
    fn t(&self, paper: f32) -> usize {
        (paper * self.scale) as usize
    }
}

impl Effect for Plate {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [
            predelay,
            decay,
            damping,
            bandwidth,
            diffusion,
            modulation,
            low_cut,
            width,
            mix,
            freeze,
        ] = self.values;
        let frozen = freeze >= 0.5;
        let decay = if frozen { 1.0 } else { decay_gain(decay) };
        let damp = if frozen {
            1.0
        } else {
            lowpass_coefficient(damping, self.rate)
        };
        let band = lowpass_coefficient(bandwidth, self.rate);
        let d = diffusion / 100.0;
        let (input_1, input_2) = (0.75 * d, 0.625 * d);
        let (decay_1, decay_2) = (0.7 * d, (decay + 0.15).clamp(0.25, 0.5) * d.max(0.3));
        let excursion = self.excursion * modulation / 100.0;
        let lfo_step = std::f32::consts::TAU * 0.9 / self.rate;
        let pre = predelay * 0.001 * self.rate;
        let hp = pole(low_cut, self.rate);
        let (wet, dry) = (mix / 100.0, 1.0 - mix / 100.0);
        let w = width / 100.0;
        let taps = [
            [266.0, 2974.0, 1913.0, 1996.0, 1990.0, 187.0, 1066.0],
            [353.0, 3627.0, 1228.0, 2673.0, 2111.0, 335.0, 121.0],
        ]
        .map(|side| side.map(|p| self.t(p)));
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = if frozen { 0.0 } else { 0.5 * (*l + *r) };
            self.predelay.push(input);
            let x = self.predelay.read(pre.max(1.0));
            self.input_lp += band * (x - self.input_lp);
            let mut x = self.input_lp;
            x = self.diffusers[0].process(x, input_1, 0.0);
            x = self.diffusers[1].process(x, input_1, 0.0);
            x = self.diffusers[2].process(x, input_2, 0.0);
            x = self.diffusers[3].process(x, input_2, 0.0);
            x = self.diffusers[4].process(x, input_2, 0.0);
            x = self.diffusers[5].process(x, input_2, 0.0);

            self.lfo = (self.lfo + lfo_step) % std::f32::consts::TAU;
            let offsets = [self.lfo.sin() * excursion, self.lfo.cos() * excursion];
            // Each half takes the input plus the other half's output.
            let feedback = [
                self.delay_b[1].tap(self.delay_b[1].len() - 1),
                self.delay_b[0].tap(self.delay_b[0].len() - 1),
            ];
            for side in 0..2 {
                let mut v = x + decay * feedback[side];
                v = self.mod_ap[side].process(v, -decay_1, offsets[side]);
                self.delay_a[side].push(v);
                let delayed = self.delay_a[side].tap(self.delay_a[side].len() - 1);
                self.damp_state[side] += damp * (delayed - self.damp_state[side]);
                let v = self.damp_state[side] * decay;
                let v = self.ap2[side].process(v, decay_2, 0.0);
                self.delay_b[side].push(v);
            }

            // Output taps, as in the paper: left reads mostly the right half and vice versa.
            let (da, db, a2) = (&self.delay_a, &self.delay_b, &self.ap2);
            let [tl, tr] = &taps;
            let out_l = 0.6
                * (da[1].tap(tl[0]) + da[1].tap(tl[1]) - a2[1].line.tap(tl[2]) + db[1].tap(tl[3])
                    - da[0].tap(tl[4])
                    - a2[0].line.tap(tl[5])
                    - db[0].tap(tl[6]));
            let out_r = 0.6
                * (da[0].tap(tr[0]) + da[0].tap(tr[1]) - a2[0].line.tap(tr[2]) + db[0].tap(tr[3])
                    - da[1].tap(tr[4])
                    - a2[1].line.tap(tr[5])
                    - db[1].tap(tr[6]));
            // Width around the mid, then a gentle low cut on the tail.
            let mid = 0.5 * (out_l + out_r);
            let side = 0.5 * (out_l - out_r) * w;
            let mut wet_out = [mid + side, mid - side];
            for (ch, s) in wet_out.iter_mut().enumerate() {
                let y = hp * (self.hp[ch] + *s - self.hp_in[ch]);
                self.hp_in[ch] = *s;
                self.hp[ch] = y;
                *s = y;
            }
            *l = *l * dry + wet_out[0] * wet;
            *r = *r * dry + wet_out[1] * wet;
        }
    }

    fn reset(&mut self) {
        self.predelay.clear();
        self.input_lp = 0.0;
        for a in self
            .diffusers
            .iter_mut()
            .chain(self.mod_ap.iter_mut())
            .chain(self.ap2.iter_mut())
        {
            a.line.clear();
        }
        for l in self.delay_a.iter_mut().chain(self.delay_b.iter_mut()) {
            l.clear();
        }
        self.damp_state = [0.0; 2];
        self.hp = [0.0; 2];
        self.hp_in = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(plate: &mut Plate, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (48_000.0 * seconds) as usize;
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        l[0] = 1.0;
        r[0] = 1.0;
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            plate.process(cl, cr);
        }
        (l, r)
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|s| s * s).sum()
    }

    #[test]
    fn tail_is_stereo_dense_and_decays() {
        let mut plate = Plate::default();
        plate.set_param(8, 100.0);
        plate.prepare(48_000, 256);
        let (l, r) = impulse(&mut plate, 4.0);
        assert!(l.iter().chain(&r).all(|s| s.is_finite()));
        let early = energy(&l[4800..24_000]);
        let late = energy(&l[150_000..]);
        assert!(early > 0.01, "a tail: {early}");
        assert!(late < early * 0.05, "it decays: {late} vs {early}");
        let different: f32 = l[4800..48_000]
            .iter()
            .zip(&r[4800..48_000])
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(different > 1.0, "decorrelated sides");
        // Dense: few silent samples once the tail builds up.
        let quiet = l[9600..19_200].iter().filter(|s| s.abs() < 1e-5).count();
        assert!(quiet < 200, "{quiet}");
    }

    #[test]
    fn longer_decay_rings_longer_and_freeze_holds() {
        let ring = |decay: f32| {
            let mut plate = Plate::default();
            plate.set_param(1, decay);
            plate.set_param(8, 100.0);
            plate.prepare(48_000, 256);
            let (l, _) = impulse(&mut plate, 3.0);
            energy(&l[96_000..])
        };
        assert!(ring(90.0) > ring(20.0) * 10.0);
        let mut plate = Plate::default();
        plate.set_param(8, 100.0);
        plate.prepare(48_000, 256);
        let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
        l[0] = 1.0;
        plate.process(&mut l, &mut r);
        plate.set_param(9, 1.0);
        let mut last = 0.0;
        for _ in 0..100 {
            let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
            plate.process(&mut l, &mut r);
            last = energy(&l);
        }
        assert!(
            last > 1e-4 && last.is_finite(),
            "frozen tail keeps going: {last}"
        );
        let (long, short) = decay_seconds(90.0, 5000.0);
        assert!(long > short && long > 2.0);
    }
}
