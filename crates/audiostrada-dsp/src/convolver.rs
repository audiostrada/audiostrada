//! Mono convolution with an impulse response: uniformly partitioned overlap-save FFT,
//! allocation-free once built. Latency is one partition.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub const PARTITION: usize = 128;

pub struct Convolver {
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    /// Spectra of the impulse response partitions.
    filters: Vec<Vec<Complex<f32>>>,
    /// Spectra of the last inputs, newest at `head`.
    history: Vec<Vec<Complex<f32>>>,
    head: usize,
    /// Previous and current input partitions.
    window: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    sum: Vec<Complex<f32>>,
    time: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    input: Vec<f32>,
    output: Vec<f32>,
    fill: usize,
}

impl std::fmt::Debug for Convolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Convolver")
            .field("partitions", &self.filters.len())
            .finish_non_exhaustive()
    }
}

impl Convolver {
    pub fn new(impulse: &[f32]) -> Self {
        let size = 2 * PARTITION;
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let mut scratch = forward.make_scratch_vec();
        if inverse.get_scratch_len() > scratch.len() {
            scratch = inverse.make_scratch_vec();
        }
        let parts = impulse.len().div_ceil(PARTITION).max(1);
        let filters = (0..parts)
            .map(|p| {
                let mut buf = vec![0.0; size];
                let start = p * PARTITION;
                let end = (start + PARTITION).min(impulse.len());
                if start < end {
                    buf[..end - start].copy_from_slice(&impulse[start..end]);
                }
                let mut spectrum = forward.make_output_vec();
                let _ = forward.process_with_scratch(&mut buf, &mut spectrum, &mut scratch);
                spectrum
            })
            .collect();
        Self {
            history: vec![forward.make_output_vec(); parts],
            spectrum: forward.make_output_vec(),
            sum: forward.make_output_vec(),
            time: vec![0.0; size],
            window: vec![0.0; size],
            forward,
            inverse,
            filters,
            head: 0,
            scratch,
            input: vec![0.0; PARTITION],
            output: vec![0.0; PARTITION],
            fill: 0,
        }
    }

    pub fn latency(&self) -> u32 {
        PARTITION as u32
    }

    pub fn reset(&mut self) {
        for h in &mut self.history {
            h.fill(Complex::default());
        }
        self.window.fill(0.0);
        self.output.fill(0.0);
        self.fill = 0;
    }

    /// Convolves `io` in place (delayed by [`Self::latency`]).
    pub fn process(&mut self, io: &mut [f32]) {
        for s in io {
            self.input[self.fill] = *s;
            *s = self.output[self.fill];
            self.fill += 1;
            if self.fill == PARTITION {
                self.fill = 0;
                self.block();
            }
        }
    }

    fn block(&mut self) {
        let n = PARTITION;
        self.window.copy_within(n.., 0);
        self.window[n..].copy_from_slice(&self.input);
        self.time.copy_from_slice(&self.window);
        let _ = self.forward.process_with_scratch(
            &mut self.time,
            &mut self.spectrum,
            &mut self.scratch,
        );
        let parts = self.filters.len();
        self.head = (self.head + parts - 1) % parts;
        self.history[self.head].copy_from_slice(&self.spectrum);
        self.sum.fill(Complex::default());
        for (p, filter) in self.filters.iter().enumerate() {
            let x = &self.history[(self.head + p) % parts];
            for ((acc, a), b) in self.sum.iter_mut().zip(x).zip(filter) {
                *acc += a * b;
            }
        }
        if let Some(first) = self.sum.first_mut() {
            first.im = 0.0;
        }
        if let Some(last) = self.sum.last_mut() {
            last.im = 0.0;
        }
        let _ = self
            .inverse
            .process_with_scratch(&mut self.sum, &mut self.time, &mut self.scratch);
        let scale = 1.0 / (2 * n) as f32;
        for (o, t) in self.output.iter_mut().zip(&self.time[n..]) {
            *o = t * scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_direct_convolution() {
        let impulse: Vec<f32> = (0..700)
            .map(|i| ((i * 7919) % 101) as f32 / 101.0 - 0.5)
            .map(|v| v * (-(0.004f32)).exp())
            .collect();
        let input: Vec<f32> = (0..3000)
            .map(|i| ((i * 31) % 17) as f32 / 17.0 - 0.5)
            .collect();
        let mut c = Convolver::new(&impulse);
        let mut out = input.clone();
        for chunk in out.chunks_mut(97) {
            c.process(chunk);
        }
        let lag = c.latency() as usize;
        for t in (lag..3000).step_by(13) {
            let n = t - lag;
            let direct: f32 = (0..impulse.len().min(n + 1))
                .map(|k| impulse[k] * input[n - k])
                .sum();
            assert!(
                (out[t] - direct).abs() < 1e-3,
                "{t}: {} vs {direct}",
                out[t]
            );
        }
    }
}
