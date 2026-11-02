use std::slice;

/// Planar audio buffers handed to the audio callback for one cycle.
#[derive(Debug)]
pub struct AudioBuffers<'a> {
    inputs: &'a [*const f32],
    outputs: &'a [*mut f32],
    frames: usize,
}

impl<'a> AudioBuffers<'a> {
    /// # Safety
    ///
    /// Every pointer must be non-null and valid for `frames` samples during `'a`. Output buffers
    /// must not overlap each other nor any input buffer.
    pub unsafe fn from_raw(
        inputs: &'a [*const f32],
        outputs: &'a [*mut f32],
        frames: usize,
    ) -> Self {
        Self {
            inputs,
            outputs,
            frames,
        }
    }

    /// Number of frames in this cycle; may change from one cycle to the next.
    #[inline]
    pub fn frames(&self) -> usize {
        self.frames
    }

    #[inline]
    pub fn input_channels(&self) -> usize {
        self.inputs.len()
    }

    #[inline]
    pub fn output_channels(&self) -> usize {
        self.outputs.len()
    }

    #[inline]
    pub fn input(&self, channel: usize) -> &[f32] {
        // SAFETY: guaranteed by `from_raw`.
        unsafe { slice::from_raw_parts(self.inputs[channel], self.frames) }
    }

    #[inline]
    pub fn output(&mut self, channel: usize) -> &mut [f32] {
        // SAFETY: guaranteed by `from_raw`; `&mut self` prevents aliasing through this struct.
        unsafe { slice::from_raw_parts_mut(self.outputs[channel], self.frames) }
    }

    /// Borrows an input and an output channel at once.
    #[inline]
    pub fn io(&mut self, input: usize, output: usize) -> (&[f32], &mut [f32]) {
        // SAFETY: guaranteed by `from_raw`: outputs never overlap inputs.
        unsafe {
            (
                slice::from_raw_parts(self.inputs[input], self.frames),
                slice::from_raw_parts_mut(self.outputs[output], self.frames),
            )
        }
    }

    /// Borrows two distinct output channels at once.
    #[inline]
    pub fn output_pair(&mut self, a: usize, b: usize) -> (&mut [f32], &mut [f32]) {
        assert_ne!(a, b, "output channels must differ");
        // SAFETY: guaranteed by `from_raw`; distinct output channels never overlap.
        unsafe {
            (
                slice::from_raw_parts_mut(self.outputs[a], self.frames),
                slice::from_raw_parts_mut(self.outputs[b], self.frames),
            )
        }
    }

    /// Copies output channel `src` into output channel `dst`.
    pub fn copy_output(&mut self, src: usize, dst: usize) {
        if src != dst {
            let (from, to) = (self.outputs[src], self.outputs[dst]);
            // SAFETY: distinct output channels never overlap and hold `frames` samples.
            unsafe { std::ptr::copy_nonoverlapping(from, to, self.frames) };
        }
    }

    pub fn clear_outputs(&mut self) {
        for channel in 0..self.output_channels() {
            self.output(channel).fill(0.0);
        }
    }
}

/// Owned planar buffers, used by backends for scratch space and by tests.
#[derive(Debug)]
pub struct ChannelBuffers {
    inputs: Vec<Box<[f32]>>,
    outputs: Vec<Box<[f32]>>,
    input_ptrs: Vec<*const f32>,
    output_ptrs: Vec<*mut f32>,
    max_frames: usize,
}

// SAFETY: the raw pointers only point into the boxed slices owned by the struct itself.
unsafe impl Send for ChannelBuffers {}

impl ChannelBuffers {
    pub fn new(inputs: usize, outputs: usize, max_frames: usize) -> Self {
        let mut inputs: Vec<Box<[f32]>> = (0..inputs)
            .map(|_| vec![0.0; max_frames].into_boxed_slice())
            .collect();
        let mut outputs: Vec<Box<[f32]>> = (0..outputs)
            .map(|_| vec![0.0; max_frames].into_boxed_slice())
            .collect();
        let input_ptrs = inputs.iter_mut().map(|b| b.as_ptr()).collect();
        let output_ptrs = outputs.iter_mut().map(|b| b.as_mut_ptr()).collect();
        Self {
            inputs,
            outputs,
            input_ptrs,
            output_ptrs,
            max_frames,
        }
    }

    pub fn max_frames(&self) -> usize {
        self.max_frames
    }

    pub fn input_mut(&mut self, channel: usize) -> &mut [f32] {
        &mut self.inputs[channel]
    }

    pub fn output(&self, channel: usize) -> &[f32] {
        &self.outputs[channel]
    }

    /// Raw pointer to an output channel, for backends substituting missing device buffers.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn output_ptr(&mut self, channel: usize) -> *mut f32 {
        self.output_ptrs[channel]
    }

    /// Views the first `frames` frames as callback buffers.
    pub fn buffers(&mut self, frames: usize) -> AudioBuffers<'_> {
        assert!(frames <= self.max_frames);
        // SAFETY: pointers target distinct owned slices of `max_frames` samples, borrowed mutably.
        unsafe { AudioBuffers::from_raw(&self.input_ptrs, &self.output_ptrs, frames) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_are_independent() {
        let mut storage = ChannelBuffers::new(1, 2, 8);
        storage.input_mut(0)[..4].copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
        let mut buffers = storage.buffers(4);
        assert_eq!(buffers.frames(), 4);
        let input: Vec<f32> = buffers.input(0).to_vec();
        buffers.output(0).copy_from_slice(&input);
        buffers.output(1).fill(-1.0);
        buffers.copy_output(0, 1);
        assert_eq!(&storage.output(0)[..4], &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(&storage.output(1)[..4], &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(storage.output(1)[4], 0.0);
    }

    #[test]
    fn paired_borrows_and_clearing() {
        let mut storage = ChannelBuffers::new(1, 2, 4);
        assert_eq!(storage.max_frames(), 4);
        storage.input_mut(0).copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
        let mut buffers = storage.buffers(3);
        assert_eq!(
            (buffers.input_channels(), buffers.output_channels()),
            (1, 2)
        );
        let (input, output) = buffers.io(0, 1);
        output.copy_from_slice(input);
        let (a, b) = buffers.output_pair(0, 1);
        a.copy_from_slice(b);
        b.fill(9.0);
        buffers.copy_output(1, 1);
        assert_eq!(buffers.output(0), &[1.0, 2.0, 3.0]);
        buffers.clear_outputs();
        assert_eq!(buffers.output(1), &[0.0; 3]);
    }
}
