//! Utility: gain, stereo width, mono, polarity per side, channel swap.

use crate::{Effect, EffectInfo, db_to_gain, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "utility",
    name: "Utility",
    params: &[
        param("gain", "Gain", -48.0, 24.0, 0.0, "dB"),
        param("width", "Width", 0.0, 200.0, 100.0, "%"),
        switch("mono", "Mono", 0.0),
        switch("invert_left", "Invert left", 0.0),
        switch("invert_right", "Invert right", 0.0),
        switch("swap", "Swap channels", 0.0),
    ],
    meters: &[],
    instrument: false,
    files: &[],
    category: "Utility",
    description: "Gain, width, mono, polarity and channel swap.",
};

#[derive(Debug)]
pub struct Utility {
    gain: f32,
    width: f32,
    mono: bool,
    invert: [bool; 2],
    swap: bool,
    /// Gains applied at the end of the previous block, for ramps.
    current: Option<[f32; 4]>,
}

impl Default for Utility {
    fn default() -> Self {
        Self {
            gain: 1.0,
            width: 1.0,
            mono: false,
            invert: [false; 2],
            swap: false,
            current: None,
        }
    }
}

impl Utility {
    /// Output = matrix * (left, right): [ll, lr, rl, rr].
    fn matrix(&self) -> [f32; 4] {
        let width = if self.mono { 0.0 } else { self.width };
        // Mid/side: mid = (l + r) / 2, side = (l - r) / 2 * width.
        let (a, b) = ((1.0 + width) / 2.0, (1.0 - width) / 2.0);
        let mut m = [a, b, b, a];
        if self.swap {
            m = [m[2], m[3], m[0], m[1]];
        }
        let sign = |inv: bool| if inv { -self.gain } else { self.gain };
        let (gl, gr) = (sign(self.invert[0]), sign(self.invert[1]));
        [m[0] * gl, m[1] * gl, m[2] * gr, m[3] * gr]
    }
}

impl Effect for Utility {
    fn prepare(&mut self, _sample_rate: u32, _max_frames: usize) {
        self.current = None;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.gain = db_to_gain(value),
            1 => self.width = value / 100.0,
            2 => self.mono = value >= 0.5,
            3 => self.invert[0] = value >= 0.5,
            4 => self.invert[1] = value >= 0.5,
            5 => self.swap = value >= 0.5,
            _ => {}
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let target = self.matrix();
        let start = self.current.unwrap_or(target);
        self.current = Some(target);
        let n = left.len().min(right.len());
        let step = 1.0 / n.max(1) as f32;
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let t = (i + 1) as f32 * step;
            let m: [f32; 4] = std::array::from_fn(|k| start[k] + (target[k] - start[k]) * t);
            let (x, y) = (*l, *r);
            *l = m[0] * x + m[1] * y;
            *r = m[2] * x + m[3] * y;
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
        u.set_param(9, 1.0);
        let (mut a, mut b) = ([1.0f32; 4], [0.0f32; 4]);
        u.process(&mut a, &mut b);
        assert_eq!(a, [0.5, 0.0, -0.5, -1.0], "from +1 to -1 in four steps");
        u.prepare(48_000, 4);
        assert_eq!(u.current, None, "a new stream starts on the target");
    }
}
