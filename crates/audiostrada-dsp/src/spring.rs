//! Spring reverb engine: two springs, each a delay loop through a chain of first-order
//! all-passes whose dispersion smears every echo into the chirp ("boing") of a spring tank,
//! band-limited like the transducers of a real one.

use crate::line::Line;
use crate::{lowpass_coefficient, pole};

const STAGES: usize = 40;
const LENGTHS_MS: [f32; 2] = [37.3, 43.1];
const MAX_PREDELAY_MS: f32 = 200.0;
/// Lowest frequency the springs carry.
const TANK_LOW_HZ: f32 = 120.0;

#[derive(Debug, Clone, Copy)]
struct Side {
    /// All-pass chain states: last input and output of each stage.
    stages: [(f32, f32); STAGES],
    damp: f32,
    hp: (f32, f32),
}

impl Default for Side {
    fn default() -> Self {
        Self {
            stages: [(0.0, 0.0); STAGES],
            damp: 0.0,
            hp: (0.0, 0.0),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Spring {
    rate: f32,
    predelay: Line,
    loops: [Line; 2],
    lengths: [usize; 2],
    sides: [Side; 2],
    out_hp: [(f32, f32); 2],
    out_lp: [f32; 2],
    pub decay: f32,
    pub predelay_ms: f32,
    pub low_cut: f32,
    pub high_cut: f32,
    pub width: f32,
    /// Treble decay, 10 to 100 %: how fast the highs die in the loop.
    pub treble: f32,
    /// Dispersion of the chirp, 0 to 100 %.
    pub diffusion: f32,
    pub freeze: bool,
}

impl Default for Spring {
    fn default() -> Self {
        let mut spring = Self {
            rate: 48_000.0,
            predelay: Line::default(),
            loops: Default::default(),
            lengths: [0; 2],
            sides: [Side::default(); 2],
            out_hp: [(0.0, 0.0); 2],
            out_lp: [0.0; 2],
            decay: 2.0,
            predelay_ms: 0.0,
            low_cut: 20.0,
            high_cut: 20_000.0,
            width: 100.0,
            treble: 45.0,
            diffusion: 80.0,
            freeze: false,
        };
        spring.prepare(48_000);
        spring
    }
}

impl Spring {
    pub(crate) fn prepare(&mut self, rate: u32) {
        self.rate = rate as f32;
        let samples = |ms: f32| (ms * 0.001 * self.rate) as usize;
        self.predelay = Line::new(samples(MAX_PREDELAY_MS) + 4);
        self.lengths = LENGTHS_MS.map(samples);
        self.loops = self.lengths.map(|l| Line::new(l + 2));
        self.reset();
    }

    pub(crate) fn reset(&mut self) {
        self.predelay.clear();
        for l in &mut self.loops {
            l.clear();
        }
        self.sides = [Side::default(); 2];
        self.out_hp = [(0.0, 0.0); 2];
        self.out_lp = [0.0; 2];
    }

    /// Replaces `left` and `right` with the springs' sound alone.
    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let a = 0.35 + 0.4 * self.diffusion / 100.0;
        let damp = if self.freeze {
            1.0
        } else {
            lowpass_coefficient(2000.0 + (self.treble - 10.0) / 90.0 * 6000.0, rate)
        };
        let tank_hp = if self.freeze {
            1.0
        } else {
            pole(TANK_LOW_HZ, rate)
        };
        let gains = self.lengths.map(|l| {
            if self.freeze {
                1.0
            } else {
                10f32.powf(-3.0 * l as f32 / rate / self.decay.max(0.05))
            }
        });
        let pre = (self.predelay_ms * 0.001 * rate).max(1.0) as usize;
        let (out_hp, out_lp) = (
            pole(self.low_cut, rate),
            lowpass_coefficient(self.high_cut, rate),
        );
        let w = self.width / 100.0;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = if self.freeze { 0.0 } else { 0.5 * (*l + *r) };
            self.predelay.push(input);
            let x = self.predelay.tap(pre);
            let mut out = [0.0f32; 2];
            for (s, side) in self.sides.iter_mut().enumerate() {
                let mut v = self.loops[s].tap(self.lengths[s]);
                for (x1, y1) in side.stages.iter_mut() {
                    let y = a * (v - *y1) + *x1;
                    (*x1, *y1) = (v, y);
                    v = y;
                }
                side.damp += damp * (v - side.damp);
                // The tank passes no deep lows, so they cannot build up in the loop.
                let (h1, hy) = side.hp;
                let hp = tank_hp * (hy + side.damp - h1);
                side.hp = (side.damp, hp);
                out[s] = hp;
                self.loops[s].push(x + gains[s] * hp);
            }
            let mid = 0.5 * (out[0] + out[1]);
            let side = 0.5 * (out[0] - out[1]) * w;
            let wet = [mid + side, mid - side];
            for (ch, s) in [l, r].into_iter().enumerate() {
                let (x1, y1) = self.out_hp[ch];
                let y = out_hp * (y1 + wet[ch] - x1);
                self.out_hp[ch] = (wet[ch], y);
                self.out_lp[ch] += out_lp * (y - self.out_lp[ch]);
                *s = self.out_lp[ch];
            }
        }
    }
}
