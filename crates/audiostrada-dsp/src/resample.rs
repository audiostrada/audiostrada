//! Streaming windowed-sinc sample rate converter, allocation-free once built.

const HALF: usize = 12;
/// Input samples a converter holds back before it can produce an output.
pub const LOOKAHEAD: usize = HALF;
const PHASES: usize = 512;

#[derive(Debug, Clone)]
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Time of the next output, in input samples from `buf[0]`.
    time: f64,
    buf: Vec<f32>,
    len: usize,
    /// `PHASES + 1` rows of `2 * HALF` taps.
    table: Vec<f32>,
}

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 1..32 {
        term *= (x / (2.0 * k as f64)).powi(2);
        sum += term;
    }
    sum
}

impl Resampler {
    /// Converts from `from` Hz to `to` Hz, taking at most `max_input` samples per call.
    pub fn new(from: u32, to: u32, max_input: usize) -> Self {
        let step = f64::from(from) / f64::from(to.max(1));
        // Below the lower Nyquist, with some room for the transition band.
        let cutoff = (1.0 / step).min(1.0) * 0.91;
        let beta = 8.0;
        let mut table = vec![0.0; (PHASES + 1) * 2 * HALF];
        for p in 0..=PHASES {
            let frac = p as f64 / PHASES as f64;
            for j in 0..2 * HALF {
                let d = frac + (HALF - 1) as f64 - j as f64;
                let x = d * cutoff * std::f64::consts::PI;
                let sinc = if x.abs() < 1e-9 { 1.0 } else { x.sin() / x };
                let w = d / HALF as f64;
                let window = if w.abs() >= 1.0 {
                    0.0
                } else {
                    bessel_i0(beta * (1.0 - w * w).sqrt()) / bessel_i0(beta)
                };
                table[p * 2 * HALF + j] = (cutoff * sinc * window) as f32;
            }
        }
        Self {
            step,
            time: HALF as f64,
            buf: vec![0.0; max_input + 4 * HALF],
            len: HALF,
            table,
        }
    }

    /// Outputs lag their input by this many output samples while streaming.
    pub fn latency(&self) -> usize {
        (HALF as f64 / self.step).ceil() as usize
    }

    /// Most outputs a call with `input` samples can produce.
    pub fn max_output(&self, input: usize) -> usize {
        ((input + 2 * HALF) as f64 / self.step).ceil() as usize + 2
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.len = HALF;
        self.time = HALF as f64;
    }

    /// Converts `input`, writing as many outputs as are ready; returns how many.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) -> usize {
        let mut written = 0;
        for chunk in input.chunks(self.buf.len() - self.len) {
            self.buf[self.len..self.len + chunk.len()].copy_from_slice(chunk);
            self.len += chunk.len();
            while written < out.len() {
                let i = self.time.floor() as usize;
                if i + HALF >= self.len {
                    break;
                }
                let frac = self.time - i as f64;
                let p = (frac * PHASES as f64).round() as usize;
                let taps = &self.table[p * 2 * HALF..(p + 1) * 2 * HALF];
                let window = &self.buf[i + 1 - HALF..i + 1 + HALF];
                out[written] = taps.iter().zip(window).map(|(t, x)| t * x).sum();
                written += 1;
                self.time += self.step;
            }
            // Drop samples no output needs any more.
            let keep_from = (self.time.floor() as usize + 1).saturating_sub(HALF);
            if keep_from > 0 {
                self.buf.copy_within(keep_from..self.len, 0);
                self.len -= keep_from;
                self.time -= keep_from as f64;
            }
        }
        written
    }

    /// Converts a whole signal at once (not real-time).
    pub fn convert(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
        if from == to {
            return samples.to_vec();
        }
        let mut r = Resampler::new(from, to, 4096);
        let mut out = Vec::new();
        let mut block = vec![0.0; r.max_output(4096)];
        let tail = vec![0.0; 2 * HALF];
        for chunk in samples.chunks(4096).chain(std::iter::once(tail.as_slice())) {
            let n = r.process(chunk, &mut block);
            out.extend_from_slice(&block[..n]);
        }
        let wanted = (samples.len() as f64 / r.step).round() as usize;
        out.truncate(wanted);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, f: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (i as f32 * f * std::f32::consts::TAU / rate as f32).sin())
            .collect()
    }

    #[test]
    fn converts_rates_without_distortion() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (48_000, 96_000)] {
            let input = sine(from, 1000.0, 20_000);
            let out = Resampler::convert(&input, from, to);
            let expected = sine(to, 1000.0, out.len());
            assert!((out.len() as f64 - 20_000.0 * f64::from(to) / f64::from(from)).abs() < 2.0);
            let error = out[500..out.len() - 500]
                .iter()
                .zip(&expected[500..])
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(error < 0.01, "{from} -> {to}: {error}");
        }
    }

    #[test]
    fn streams_blocks_of_any_size() {
        let input = sine(44_100, 440.0, 8000);
        let mut r = Resampler::new(44_100, 48_000, 512);
        let mut out = Vec::new();
        let mut block = vec![0.0; r.max_output(512)];
        let mut at = 0;
        for size in [1, 7, 512, 300, 64].iter().cycle() {
            if at >= input.len() {
                break;
            }
            let end = (at + size).min(input.len());
            let n = r.process(&input[at..end], &mut block);
            out.extend_from_slice(&block[..n]);
            at = end;
        }
        let whole = Resampler::convert(&input, 44_100, 48_000);
        let error = whole[100..6000]
            .iter()
            .zip(&out[100..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(error < 1e-4, "{error}");
    }
}
