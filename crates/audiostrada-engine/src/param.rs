use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// A float parameter shared lock-free between the UI and the audio thread.
#[derive(Clone, Debug)]
pub struct SharedParam(Arc<AtomicU32>);

impl SharedParam {
    pub fn new(value: f32) -> Self {
        Self(Arc::new(AtomicU32::new(value.to_bits())))
    }

    #[inline]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// Linear per-block ramp towards a target, to avoid zipper noise. The first block starts
/// directly at its target, so renders are exact from the first sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct Smoothed {
    current: Option<f32>,
}

impl Smoothed {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns (start, step) to ramp from the current value to `target` over `frames`.
    #[inline]
    pub fn ramp(&mut self, target: f32, frames: usize) -> (f32, f32) {
        let start = self.current.unwrap_or(target);
        self.current = Some(target);
        let step = if frames == 0 {
            0.0
        } else {
            (target - start) / frames as f32
        };
        (start, step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_param_round_trip() {
        let p = SharedParam::new(0.5);
        let q = p.clone();
        q.set(-3.25);
        assert_eq!(p.get(), -3.25);
    }

    #[test]
    fn ramp_reaches_target() {
        let mut s = Smoothed::new();
        assert_eq!(
            s.ramp(0.0, 4),
            (0.0, 0.0),
            "first block starts at its target"
        );
        let (start, step) = s.ramp(1.0, 4);
        assert_eq!((start, step), (0.0, 0.25));
        assert_eq!(s.ramp(1.0, 4), (1.0, 0.0));
    }
}
