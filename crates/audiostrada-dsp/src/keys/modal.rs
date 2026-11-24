//! Modes of vibration as decaying sine resonators (the coupled form: a rotation scaled by the
//! decay, which keeps its amplitude when the frequency moves), and the hammer that sets them
//! going.

use std::f32::consts::TAU;

/// One mode: its state (position and velocity in the rotating plane), rotation and decay.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Mode {
    re: f32,
    im: f32,
    cos: f32,
    sin: f32,
    decay: f32,
}

impl Mode {
    /// Sets the frequency, keeping what the mode holds.
    pub fn tune(&mut self, hz: f32, rate: f32) {
        let w = TAU * hz.min(rate * 0.49) / rate;
        (self.sin, self.cos) = w.sin_cos();
    }

    /// Sets the time the mode takes to fall by 60 dB.
    pub fn set_t60(&mut self, seconds: f32, rate: f32) {
        self.decay = decay(seconds, rate);
    }

    /// Advances by one sample with `push` added; returns the displacement.
    #[inline]
    pub fn tick(&mut self, push: f32) -> f32 {
        let re = self.decay * (self.cos * self.re - self.sin * self.im) + push;
        self.im = self.decay * (self.sin * self.re + self.cos * self.im);
        self.re = re;
        self.im
    }

    pub fn energy(&self) -> f32 {
        self.re * self.re + self.im * self.im
    }

    pub fn clear(&mut self) {
        self.re = 0.0;
        self.im = 0.0;
    }
}

/// Factor per sample of a decay falling 60 dB in `seconds`.
pub(crate) fn decay(seconds: f32, rate: f32) -> f32 {
    10f32.powf(-3.0 / (seconds.max(1e-3) * rate))
}

/// The force of a hammer on its tine or string: a smooth pulse (two one-pole stages fed by
/// an impulse) whose length is the contact time. Shorter is brighter: its spectrum falls
/// past `1 / contact`, without the holes a windowed pulse would leave.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Hammer {
    stages: [f32; 2],
    pole: f32,
}

impl Hammer {
    /// A strike of strength `force` lasting about `contact` seconds.
    pub fn strike(&mut self, force: f32, contact: f32, rate: f32) {
        self.pole = (-3.0 / (contact.max(1e-5) * rate)).exp();
        self.stages[0] += force * (1.0 - self.pole);
    }

    /// The pulse's gain at `hz` relative to its total, for the current contact.
    pub fn response(&self, hz: f32, rate: f32) -> f32 {
        let (s, c) = (TAU * hz / rate).sin_cos();
        let p = self.pole;
        let one = (1.0 - p) * (1.0 - p);
        let den = (1.0 - p * c) * (1.0 - p * c) + p * p * s * s;
        one / den.max(1e-12)
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        let p = self.pole;
        let out = self.stages[1];
        self.stages[1] = p * self.stages[1] + (1.0 - p) * self.stages[0];
        self.stages[0] *= p;
        out
    }

    pub fn active(&self) -> bool {
        self.stages[0].abs() + self.stages[1].abs() > 1e-7
    }

    pub fn clear(&mut self) {
        self.stages = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_rings_at_its_frequency_and_falls_60_db_in_its_time() {
        let rate = 48_000.0;
        let mut mode = Mode::default();
        mode.tune(1000.0, rate);
        mode.set_t60(0.5, rate);
        mode.tick(1.0);
        let mut crossings = 0;
        let mut last = 0.0;
        for _ in 0..4800 {
            let y = mode.tick(0.0);
            if last <= 0.0 && y > 0.0 {
                crossings += 1;
            }
            last = y;
        }
        assert!((99..=101).contains(&crossings), "{crossings}");
        for _ in 4800..24_000 {
            mode.tick(0.0);
        }
        assert!((mode.energy().sqrt() - 1e-3).abs() < 2e-4);
    }

    #[test]
    fn the_hammer_pushes_its_whole_force_and_shorter_is_brighter() {
        let rate = 48_000.0;
        let mut hammer = Hammer::default();
        hammer.strike(1.0, 0.002, rate);
        let soft = hammer.response(4000.0, rate);
        let total: f32 = (0..48_000).map(|_| hammer.tick()).sum();
        assert!((total - 1.0).abs() < 1e-3);
        hammer.strike(1.0, 0.0005, rate);
        assert!(hammer.response(4000.0, rate) > soft * 4.0);
        assert!((hammer.response(0.0, rate) - 1.0).abs() < 1e-4);
    }
}
