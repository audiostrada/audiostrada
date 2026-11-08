//! Neural Amp Modeler captures (A1 WaveNet/LSTM, A2 slimmable) through `nam-rs`, run at
//! their own sample rate and resampled when the stream differs, for the Amp effect.

use nam_rs::{Model, NamModel};

use crate::resample::{LOOKAHEAD, Resampler};

/// Longest impulse response kept, in seconds.
const MAX_IR_SECONDS: f32 = 2.0;

/// One capture running at its own sample rate: the model, and the resampling and FIFO around
/// it when the stream rate differs. Processes mono audio in place.
pub(crate) struct NamCore {
    model: NamModel,
    /// Read once: the metadata is parsed (and allocated) on every query.
    loudness: Option<f32>,
    model_rate: u32,
    runtime: Option<Model>,
    rate: u32,
    up: Option<Resampler>,
    down: Option<Resampler>,
    at_model_rate: Vec<f32>,
    back: Vec<f32>,
    /// Output of the model at the stream rate, primed with silence.
    fifo: Vec<f32>,
    fifo_start: usize,
    fifo_len: usize,
    delay: usize,
    /// The model's state went non-finite: the next reset clears it.
    broken: bool,
    /// Samples run through the model since its state was blank: once it has had
    /// [`Self::warm_len`], its output no longer depends on the blank start.
    fed: usize,
}

impl NamCore {
    /// Reads a `.nam` file, checking that it can run.
    pub(crate) fn parse(bytes: Vec<u8>) -> Result<NamModel, String> {
        let text = String::from_utf8(bytes).map_err(|_| "not a NAM model".to_owned())?;
        let model = NamModel::from_json_str(&text).map_err(|e| e.to_string())?;
        Model::from_nam(&model).map_err(|e| e.to_string())?;
        Ok(model)
    }

    pub(crate) fn new(model: NamModel) -> Self {
        Self {
            model_rate: model.expected_sample_rate().round() as u32,
            loudness: model.loudness(),
            model,
            runtime: None,
            rate: 48_000,
            up: None,
            down: None,
            at_model_rate: Vec::new(),
            back: Vec::new(),
            fifo: Vec::new(),
            fifo_start: 0,
            fifo_len: 0,
            delay: 0,
            broken: false,
            fed: 0,
        }
    }

    /// The capture's loudness in dBFS, for normalization.
    pub(crate) fn loudness(&self) -> Option<f32> {
        self.loudness
    }

    /// NAM's estimate of the capture's gain, 0 to 1.
    #[cfg(test)]
    pub(crate) fn gain(&self) -> Option<f32> {
        self.model.metadata_typed().gain
    }

    pub(crate) fn prepare(&mut self, rate: u32, max_frames: usize) {
        self.rate = rate.max(1);
        let max = max_frames.max(1);
        self.runtime = Model::from_nam(&self.model).ok();
        self.up = None;
        self.down = None;
        self.delay = 0;
        if self.runtime.is_some() && self.model_rate != self.rate {
            let up = Resampler::new(self.rate, self.model_rate, max);
            let down = Resampler::new(self.model_rate, self.rate, up.max_output(max));
            self.at_model_rate = vec![0.0; up.max_output(max)];
            self.back = vec![0.0; down.max_output(up.max_output(max))];
            let held = LOOKAHEAD as f64 * (1.0 + f64::from(self.rate) / f64::from(self.model_rate));
            self.delay = held.ceil() as usize + 4;
            self.fifo = vec![0.0; self.delay + 2 * max + 64];
            self.fifo_start = 0;
            self.fifo_len = self.delay;
            self.up = Some(up);
            self.down = Some(down);
        }
        if let Some(model) = &mut self.runtime {
            model.reset();
        }
        self.fed = 0;
    }

    /// Samples the model needs from a blank state before its output is its own: from a blank
    /// state, a capture answers its first samples with a thump (up to +9 dBFS for some),
    /// then a step of its offset that the cabinet rings with.
    pub(crate) fn warm_len(&self) -> usize {
        let remembered = self.runtime.as_ref().map_or(0, Model::receptive_field) as f64
            * f64::from(self.rate)
            / f64::from(self.model_rate.max(1));
        remembered.ceil() as usize + self.delay
    }

    /// Whether the model has run long enough since its state was blank (see
    /// [`Self::warm_len`]).
    pub(crate) fn is_warm(&self) -> bool {
        self.fed >= self.warm_len()
    }

    /// Runs the model on silence until it is warm (not in real time: when preparing).
    pub(crate) fn warm(&mut self, max: usize) {
        let mut silence = vec![0.0f32; max.max(1)];
        while self.runtime.is_some() && !self.is_warm() {
            silence.fill(0.0);
            self.process(&mut silence);
        }
    }

    /// Delay added by resampling, in samples.
    pub(crate) fn latency(&self) -> usize {
        self.delay
    }

    /// Keeps the model's state, which forgets the past by itself within
    /// [`Self::warm_len`]: a blank one would thump at each start of playback. Only a state
    /// gone non-finite is cleared.
    pub(crate) fn reset(&mut self) {
        if self.broken
            && let Some(m) = &mut self.runtime
        {
            m.reset();
            self.broken = false;
            self.fed = 0;
        }
    }

    /// Runs the model on `buffer` (at most the prepared block size) in place.
    pub(crate) fn process(&mut self, buffer: &mut [f32]) {
        let Some(model) = self.runtime.as_mut() else {
            return;
        };
        match (&mut self.up, &mut self.down) {
            (Some(up), Some(down)) => {
                let m = up.process(buffer, &mut self.at_model_rate);
                model.process_buffer(&mut self.at_model_rate[..m]);
                let k = down.process(&self.at_model_rate[..m], &mut self.back);
                let cap = self.fifo.len();
                for &v in &self.back[..k] {
                    if self.fifo_len == cap {
                        break;
                    }
                    self.fifo[(self.fifo_start + self.fifo_len) % cap] = v;
                    self.fifo_len += 1;
                }
                for s in buffer.iter_mut() {
                    if self.fifo_len == 0 {
                        *s = 0.0;
                        continue;
                    }
                    *s = self.fifo[self.fifo_start];
                    self.fifo_start = (self.fifo_start + 1) % cap;
                    self.fifo_len -= 1;
                }
            }
            _ => model.process_buffer(buffer),
        }
        self.fed = self.fed.saturating_add(buffer.len());
        // A non-finite value stays in the model's state: noted for the next reset.
        if buffer.last().is_some_and(|s| !s.is_finite()) {
            self.broken = true;
        }
    }
}

/// Mono impulse response normalized to unit energy, at most `MAX_IR_SECONDS` long.
pub(crate) fn impulse(channels: &[Vec<f32>], rate: u32) -> Result<Vec<f32>, String> {
    let frames = channels.iter().map(Vec::len).max().unwrap_or(0);
    let frames = frames.min((MAX_IR_SECONDS * rate as f32) as usize);
    if frames == 0 {
        return Err("the impulse response is empty".into());
    }
    let mut mono: Vec<f32> = (0..frames)
        .map(|i| {
            channels
                .iter()
                .map(|c| c.get(i).copied().unwrap_or(0.0))
                .sum::<f32>()
                / channels.len() as f32
        })
        .collect();
    let energy = mono.iter().map(|s| s * s).sum::<f32>().sqrt();
    if energy > 1e-9 {
        mono.iter_mut().for_each(|s| *s /= energy);
    }
    Ok(mono)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-layer WaveNet computing a fixed nonlinear function (from the nam-rs docs).
    const TINY: &str = r#"{
        "version": "0.5.4", "architecture": "WaveNet",
        "config": {
            "layers": [{
                "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                "kernel_size": 1, "dilations": [1], "activation": "ReLU",
                "gated": false, "head_bias": false
            }],
            "head": null, "head_scale": 1.0
        },
        "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]
    }"#;

    fn render(core: &mut NamCore, blocks: usize, frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for b in 0..blocks {
            let mut block: Vec<f32> = (0..frames)
                .map(|i| (((b * frames + i) as f32) * 0.03).sin() * 0.5)
                .collect();
            core.process(&mut block);
            out.extend(block);
        }
        out
    }

    #[test]
    fn plays_real_models_when_available() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_NAM") else {
            eprintln!("AUDIOSTRADA_TEST_NAM not set, skipping");
            return;
        };
        let mut played = 0;
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "nam") {
                continue;
            }
            for rate in [48_000, 44_100] {
                let model = NamCore::parse(std::fs::read(&path).unwrap()).unwrap();
                let mut core = NamCore::new(model);
                core.prepare(rate, 256);
                let start = std::time::Instant::now();
                let out = render(&mut core, rate as usize / 256, 256);
                let elapsed = start.elapsed().as_secs_f64();
                assert!(out.iter().all(|s| s.is_finite()), "{}", path.display());
                let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
                assert!(rms > 1e-4, "{} silent", path.display());
                eprintln!(
                    "{} at {rate} Hz: rms {rms:.3}, {:.1}x real time",
                    path.file_name().unwrap().to_string_lossy(),
                    1.0 / elapsed
                );
                played += 1;
            }
        }
        assert!(played > 0);
    }

    #[test]
    fn runs_models_at_their_rate() {
        assert!(NamCore::parse(b"nope".to_vec()).is_err());
        let mut core = NamCore::new(NamCore::parse(TINY.as_bytes().to_vec()).unwrap());
        core.prepare(48_000, 256);
        assert_eq!(core.latency(), 0, "same rate: no resampling");
        let same = render(&mut core, 20, 256);
        assert!(same.iter().any(|s| s.abs() > 0.01));
        assert!(same.iter().all(|s| s.is_finite()));

        core.prepare(44_100, 256);
        assert!(core.latency() > 0, "resampled to 48 kHz and back");
        let resampled = render(&mut core, 20, 200);
        assert!(resampled[1000..].iter().any(|s| s.abs() > 0.01));
        assert!(resampled.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn impulses_are_mono_and_normalized() {
        assert!(impulse(&[], 48_000).is_err());
        let ir = impulse(&[vec![1.0, 0.5], vec![1.0, 0.5, 0.25]], 48_000).unwrap();
        assert_eq!(ir.len(), 3);
        let energy: f32 = ir.iter().map(|s| s * s).sum();
        assert!((energy - 1.0).abs() < 1e-5);
    }
}
