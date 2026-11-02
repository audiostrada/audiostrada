//! Recording: the audio thread copies hardware inputs into lock-free rings, a writer thread
//! streams them to WAV files. A jump in the timeline (loop wrap) starts a new take.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use audiostrada_core::TrackId;
use audiostrada_io::AudioBuffers;
use audiostrada_media::WavWriter;
use audiostrada_rt::{Receiver, Sender, channel};

const RING_SECONDS: usize = 4;
const HEADER_CAPACITY: usize = 8192;
/// Frames per peak of the live waveform.
pub const LIVE_BLOCK: u32 = 256;

/// Waveform of the take being written for one track, to draw it while recording.
#[derive(Debug)]
struct LiveTake {
    track: TrackId,
    /// Engine position of the take's first frame.
    start: u64,
    /// Highest absolute sample of each block of `LIVE_BLOCK` frames.
    peaks: Vec<f32>,
    current: f32,
    filled: u32,
}

type Live = Arc<std::sync::Mutex<Vec<LiveTake>>>;

#[derive(Clone, Copy, Debug)]
struct Header {
    position: u64,
    frames: u32,
}

/// Audio-thread side of a recording.
pub(crate) struct Capture {
    /// Hardware input channels, in ring order.
    channels: Box<[usize]>,
    punch: Option<(u64, u64)>,
    samples: Sender<f32>,
    headers: Sender<Header>,
    dropped: Arc<AtomicU64>,
}

impl Capture {
    /// Copies the frames of a segment that fall inside the punch range.
    pub(crate) fn write(
        &mut self,
        buffers: &AudioBuffers<'_>,
        offset: usize,
        frames: usize,
        position: u64,
    ) {
        let (mut start, mut end) = (position, position + frames as u64);
        if let Some((a, b)) = self.punch {
            start = start.max(a);
            end = end.min(b);
        }
        if end <= start {
            return;
        }
        let n = (end - start) as usize;
        let from = offset + (start - position) as usize;
        if self.samples.free_slots() < n * self.channels.len() || self.headers.free_slots() == 0 {
            self.dropped.fetch_add(n as u64, Ordering::Relaxed);
            return;
        }
        let inputs = buffers.input_channels();
        for i in from..from + n {
            for &ch in self.channels.iter() {
                let sample = if ch < inputs {
                    buffers.input(ch)[i]
                } else {
                    0.0
                };
                let _ = self.samples.try_send(sample);
            }
        }
        let _ = self.headers.try_send(Header {
            position: start,
            frames: n as u32,
        });
    }
}

/// A track to record and the hardware inputs feeding it (one or two).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeTarget {
    pub track: TrackId,
    pub name: String,
    pub inputs: Vec<usize>,
}

/// A recorded file; `start` is in engine samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Take {
    pub track: TrackId,
    pub path: PathBuf,
    pub start: u64,
    pub frames: u64,
    pub channels: u16,
}

#[derive(Debug, Default)]
pub struct RecordResult {
    pub takes: Vec<Take>,
    /// Frames lost because the disk could not keep up.
    pub dropped: u64,
    pub errors: Vec<String>,
}

/// UI-side handle of a recording in progress.
#[derive(Debug)]
pub struct Recorder {
    handle: Option<JoinHandle<RecordResult>>,
    dropped: Arc<AtomicU64>,
    live: Live,
}

impl Recorder {
    /// True once the audio thread let go of the capture and every file is closed.
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Calls `each` with the track, start (engine samples) and peaks of every take being
    /// written; each peak covers [`LIVE_BLOCK`] frames.
    pub fn live(&self, mut each: impl FnMut(TrackId, u64, &[f32])) {
        if let Ok(live) = self.live.lock() {
            for take in live.iter() {
                each(take.track, take.start, &take.peaks);
            }
        }
    }

    /// Waits for the writer thread and returns the takes.
    pub fn finish(mut self) -> RecordResult {
        let mut result = match self.handle.take().map(JoinHandle::join) {
            Some(Ok(result)) => result,
            Some(Err(_)) => RecordResult {
                errors: vec!["the recording thread crashed".into()],
                ..RecordResult::default()
            },
            None => RecordResult::default(),
        };
        result.dropped = self.dropped();
        result
    }
}

/// Builds both sides of a recording and starts the writer thread.
pub(crate) fn start(
    targets: Vec<TakeTarget>,
    dir: PathBuf,
    sample_rate: u32,
    punch: Option<(u64, u64)>,
) -> std::io::Result<(Box<Capture>, Recorder)> {
    std::fs::create_dir_all(&dir)?;
    let mut channels: Vec<usize> = targets
        .iter()
        .flat_map(|t| t.inputs.iter().copied())
        .collect();
    channels.sort_unstable();
    channels.dedup();
    let capacity = sample_rate as usize * channels.len().max(1) * RING_SECONDS;
    let (samples, samples_rx) = channel(capacity);
    let (headers, headers_rx) = channel(HEADER_CAPACITY);
    let dropped = Arc::new(AtomicU64::new(0));
    let routes: Vec<Vec<usize>> = targets
        .iter()
        .map(|t| {
            t.inputs
                .iter()
                .filter_map(|i| channels.iter().position(|c| c == i))
                .collect()
        })
        .collect();
    let live: Live = Arc::default();
    let writer = Writer {
        live: Arc::clone(&live),
        targets,
        routes,
        dir,
        sample_rate,
        width: channels.len(),
        files: Vec::new(),
        result: RecordResult::default(),
    };
    let handle = std::thread::Builder::new()
        .name("audiostrada-recorder".into())
        .spawn(move || writer.run(samples_rx, headers_rx))?;
    let capture = Box::new(Capture {
        channels: channels.into_boxed_slice(),
        punch,
        samples,
        headers,
        dropped: Arc::clone(&dropped),
    });
    Ok((
        capture,
        Recorder {
            handle: Some(handle),
            dropped,
            live,
        },
    ))
}

struct Writer {
    live: Live,
    targets: Vec<TakeTarget>,
    /// For each target, the indices of its channels in a ring frame.
    routes: Vec<Vec<usize>>,
    dir: PathBuf,
    sample_rate: u32,
    width: usize,
    files: Vec<Option<(WavWriter, PathBuf, u64)>>,
    result: RecordResult,
}

impl Writer {
    fn run(mut self, mut samples: Receiver<f32>, mut headers: Receiver<Header>) -> RecordResult {
        let mut expected = None;
        let mut frame = vec![0.0; self.width];
        let mut out = Vec::with_capacity(2);
        loop {
            let mut idle = true;
            while let Some(header) = headers.try_recv() {
                idle = false;
                if expected != Some(header.position) {
                    self.close();
                    self.open(header.position);
                }
                let mut live = self.live.lock().ok();
                for _ in 0..header.frames {
                    for s in frame.iter_mut() {
                        *s = samples.try_recv().unwrap_or(0.0);
                    }
                    if let Some(live) = &mut live {
                        for (take, route) in live.iter_mut().zip(&self.routes) {
                            let peak = route.iter().fold(0.0f32, |m, &i| m.max(frame[i].abs()));
                            take.current = take.current.max(peak);
                            take.filled += 1;
                            if take.filled == LIVE_BLOCK {
                                take.peaks.push(take.current);
                                take.current = 0.0;
                                take.filled = 0;
                            }
                        }
                    }
                    for (file, route) in self.files.iter_mut().zip(&self.routes) {
                        let Some((writer, path, _)) = file else {
                            continue;
                        };
                        out.clear();
                        out.extend(route.iter().map(|&i| frame[i]));
                        if let Err(e) = writer.write_frame(&out) {
                            self.result.errors.push(format!("{}: {e}", path.display()));
                            *file = None;
                        }
                    }
                }
                drop(live);
                expected = Some(header.position + u64::from(header.frames));
            }
            if idle {
                if headers.is_disconnected() && headers.is_empty() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        self.close();
        self.result
    }

    fn open(&mut self, start: u64) {
        if let Ok(mut live) = self.live.lock() {
            *live = self
                .targets
                .iter()
                .map(|t| LiveTake {
                    track: t.track,
                    start,
                    peaks: Vec::new(),
                    current: 0.0,
                    filled: 0,
                })
                .collect();
        }
        self.files = self
            .targets
            .iter()
            .map(|t| {
                let path = unique_path(&self.dir, &t.name);
                match WavWriter::create(&path, t.inputs.len().max(1) as u16, self.sample_rate) {
                    Ok(writer) => Some((writer, path, start)),
                    Err(e) => {
                        self.result.errors.push(format!("{}: {e}", path.display()));
                        None
                    }
                }
            })
            .collect();
    }

    fn close(&mut self) {
        for (file, target) in std::mem::take(&mut self.files)
            .into_iter()
            .zip(&self.targets)
        {
            let Some((writer, path, start)) = file else {
                continue;
            };
            let channels = writer.channels();
            match writer.finish() {
                Ok(0) => {
                    let _ = std::fs::remove_file(&path);
                }
                Ok(frames) => self.result.takes.push(Take {
                    track: target.track,
                    path,
                    start,
                    frames,
                    channels,
                }),
                Err(e) => self.result.errors.push(format!("{}: {e}", path.display())),
            }
        }
    }
}

/// `dir/name-NN.wav` with the first free number.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = if clean.trim().is_empty() {
        "Take".to_owned()
    } else {
        clean.trim().to_owned()
    };
    (1..)
        .map(|n| dir.join(format!("{clean}-{n:02}.wav")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{clean}.wav")))
}

#[cfg(test)]
mod tests {
    use audiostrada_io::ChannelBuffers;

    use super::*;
    use crate::{Engine, EngineConfig};

    fn record(
        name: &str,
        looping: Option<(u64, u64)>,
        punch: Option<(u64, u64)>,
    ) -> (RecordResult, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("audiostrada-record-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut engine = Engine::new(EngineConfig { workers: Some(0) });
        let mut processor = engine.processor(48_000, 64);
        let targets = vec![TakeTarget {
            track: TrackId(1),
            name: "Vox".into(),
            inputs: vec![1],
        }];
        let recorder = engine.start_recording(targets, dir.clone(), punch).unwrap();
        engine.set_loop(looping);
        engine.play();
        let mut storage = ChannelBuffers::new(2, 2, 64);
        let mut position = 0u64;
        for _ in 0..40 {
            for i in 0..64 {
                storage.input_mut(1)[i] = (position + i as u64) as f32 / 10_000.0;
            }
            processor.render(&mut storage.buffers(64));
            position += 64;
        }
        engine.stop_recording();
        processor.render(&mut storage.buffers(64));
        engine.collect_garbage();
        (recorder.finish(), dir)
    }

    #[test]
    fn loop_passes_become_takes() {
        let (result, dir) = record("loop", Some((0, 1000)), None);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.dropped, 0);
        let frames: Vec<u64> = result.takes.iter().map(|t| t.frames).collect();
        assert_eq!(frames, vec![1000, 1000, 560]);
        assert!(result.takes.iter().all(|t| t.start == 0 && t.channels == 1));
        let second = audiostrada_media::load(&result.takes[1].path).unwrap();
        assert_eq!(second.channel(0)[10], 1010.0 / 10_000.0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn live_waveform_follows_the_input() {
        let dir =
            std::env::temp_dir().join(format!("audiostrada-record-{}-live", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut engine = Engine::new(EngineConfig { workers: Some(0) });
        let mut processor = engine.processor(48_000, 64);
        let targets = vec![TakeTarget {
            track: TrackId(1),
            name: "Vox".into(),
            inputs: vec![0],
        }];
        let recorder = engine.start_recording(targets, dir.clone(), None).unwrap();
        engine.play();
        let mut storage = ChannelBuffers::new(1, 2, 64);
        for block in 0..40 {
            storage
                .input_mut(0)
                .fill(if block < 20 { 0.1 } else { -0.8 });
            processor.render(&mut storage.buffers(64));
        }
        let mut peaks = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while peaks.len() < 10 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            recorder.live(|track, start, p| {
                assert_eq!((track, start), (TrackId(1), 0));
                peaks = p.to_vec();
            });
        }
        assert_eq!(peaks.len(), 10, "2560 frames in blocks of 256");
        assert_eq!(peaks[0], 0.1);
        assert_eq!(peaks[9], 0.8);
        engine.stop_recording();
        processor.render(&mut storage.buffers(64));
        engine.collect_garbage();
        recorder.finish();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn punch_limits_the_take() {
        let (result, dir) = record("punch", None, Some((200, 500)));
        assert_eq!(result.takes.len(), 1);
        let take = &result.takes[0];
        assert_eq!((take.start, take.frames), (200, 300));
        let data = audiostrada_media::load(&take.path).unwrap();
        assert_eq!(data.channel(0)[0], 200.0 / 10_000.0);
        assert!(take.path.ends_with("Vox-01.wav"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
