//! Delay line and Schroeder all-pass shared by the delays, modulation effects and reverbs.

#[derive(Debug, Default)]
pub(crate) struct Line {
    buffer: Vec<f32>,
    write: usize,
}

impl Line {
    pub(crate) fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(2)],
            write: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub(crate) fn push(&mut self, x: f32) {
        self.buffer[self.write] = x;
        self.write = (self.write + 1) % self.buffer.len();
    }

    /// The value written `delay` samples ago (1 = the last one).
    #[inline]
    pub(crate) fn tap(&self, delay: usize) -> f32 {
        let len = self.buffer.len();
        self.buffer[(self.write + len - delay.clamp(1, len)) % len]
    }

    /// Linearly interpolated read, `delay` in samples (clamped to 1 and the length).
    #[inline]
    pub(crate) fn read(&self, delay: f32) -> f32 {
        let delay = delay.clamp(1.0, (self.buffer.len() - 1) as f32);
        let i = delay.floor();
        let frac = delay - i;
        let a = self.tap(i as usize);
        let b = self.tap(i as usize + 1);
        a + (b - a) * frac
    }

    pub(crate) fn clear(&mut self) {
        self.buffer.fill(0.0);
    }
}

/// Schroeder all-pass of `length` samples, read `offset` samples further when modulated.
#[derive(Debug, Default)]
pub(crate) struct AllPass {
    pub(crate) line: Line,
    pub(crate) length: f32,
}

impl AllPass {
    /// An all-pass of `length` samples whose modulation reaches `excursion` samples.
    pub(crate) fn new(length: f32, excursion: f32) -> Self {
        Self {
            line: Line::new((length + excursion) as usize + 4),
            length,
        }
    }

    #[inline]
    pub(crate) fn process(&mut self, x: f32, g: f32, offset: f32) -> f32 {
        let delayed = self.line.read(self.length + offset);
        let w = x + g * delayed;
        self.line.push(w);
        delayed - g * w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_and_reads_between_samples() {
        let mut line = Line::new(4);
        for x in [1.0, 2.0, 3.0] {
            line.push(x);
        }
        assert_eq!(line.tap(1), 3.0);
        assert_eq!(line.tap(3), 1.0);
        assert_eq!(line.tap(0), 3.0, "clamped to the last sample");
        assert_eq!(line.read(1.5), 2.5);
        assert_eq!(line.read(0.0), 3.0);
        line.clear();
        assert_eq!(line.tap(2), 0.0);
        assert_eq!(Line::new(0).len(), 2);
    }

    #[test]
    fn all_pass_keeps_the_energy() {
        let mut ap = AllPass::new(7.0, 0.0);
        let out: Vec<f32> = (0..2000)
            .map(|i| ap.process(if i == 0 { 1.0 } else { 0.0 }, 0.6, 0.0))
            .collect();
        let energy: f32 = out.iter().map(|s| s * s).sum();
        assert!((energy - 1.0).abs() < 1e-3, "{energy}");
        assert_eq!(out[0], -0.6);
    }
}
