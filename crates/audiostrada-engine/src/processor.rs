use audiostrada_io::AudioBuffers;

/// Transport state for the block being processed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransportInfo {
    pub playing: bool,
    /// Timeline position of the first frame of the block, in samples.
    pub position: u64,
    pub sample_rate: u32,
    /// Musical position of the first frame, in quarter-note beats.
    pub beat: f64,
    pub bpm: f64,
    pub bar: BarPosition,
    /// A count-in: the position holds and only the metronome plays, whatever its switch.
    pub counting: bool,
}

/// Where the first frame falls in its bar, in units of the time signature (eighths in 6/8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarPosition {
    /// Beat within the bar, from 1.
    pub beat: u32,
    /// Progress within that beat, 0 to 1.
    pub fraction: f64,
    pub beats_per_bar: u32,
    /// Length of one beat in quarter notes.
    pub unit: f64,
}

impl Default for BarPosition {
    fn default() -> Self {
        Self {
            beat: 1,
            fraction: 0.0,
            beats_per_bar: 4,
            unit: 1.0,
        }
    }
}

#[derive(Debug)]
pub struct ProcessContext<'a> {
    pub buffers: AudioBuffers<'a>,
    pub transport: &'a TransportInfo,
}

/// An audio node. `process` runs on a real-time thread: no allocation, no locks, no syscalls.
pub trait Processor: Send + 'static {
    /// Called off the audio thread before processing starts or when the stream format changes.
    fn prepare(&mut self, _sample_rate: u32, _max_frames: usize) {}

    fn process(&mut self, ctx: &mut ProcessContext<'_>);

    /// Called on the audio thread when the transport jumps, to clear tails and positions.
    fn reset(&mut self) {}
}

impl<F> Processor for F
where
    F: FnMut(&mut ProcessContext<'_>) + Send + 'static,
{
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        self(ctx)
    }
}
