//! Background loading of media files: decoding, peak caches and resampling to the engine rate.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use audiostrada_media::{AudioData, Peaks, load, resample};

#[derive(Clone, Debug)]
pub enum MediaState {
    Loading,
    Ready,
    Failed(String),
}

#[derive(Debug)]
struct Entry {
    state: MediaState,
    original: Option<Arc<AudioData>>,
    audio: Option<Arc<AudioData>>,
    peaks: Option<Arc<Peaks>>,
    /// Engine rate the pending or current `audio` was made for.
    rate: u32,
}

/// Decoded original, audio at the engine rate, and peaks.
type LoadOutput = (Arc<AudioData>, Arc<AudioData>, Arc<Peaks>);

struct Loaded {
    path: PathBuf,
    rate: u32,
    result: Result<LoadOutput, String>,
}

/// Decoded media keyed by absolute path, at the engine sample rate.
pub struct MediaPool {
    rate: u32,
    peaks_dir: Option<PathBuf>,
    entries: HashMap<PathBuf, Entry>,
    tx: mpsc::Sender<Loaded>,
    rx: mpsc::Receiver<Loaded>,
    generation: u64,
}

impl std::fmt::Debug for MediaPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaPool")
            .field("rate", &self.rate)
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// Cache file name for the peaks of `path`, keyed on its size and modification time.
fn peaks_cache(dir: &Path, path: &Path) -> Option<PathBuf> {
    let (file, _) = audiostrada_core::parse_variant(path);
    let meta = std::fs::metadata(&file).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    let stem = file.file_stem()?.to_string_lossy();
    Some(dir.join(format!("{stem}-{:016x}.peaks", hasher.finish())))
}

/// Decodes a file, stretched and transposed when `path` is a variant.
fn decode(path: &Path) -> Result<AudioData, String> {
    let (file, variant) = audiostrada_core::parse_variant(path);
    let data = load(&file).map_err(|e| e.to_string())?;
    let Some((stretch, pitch)) = variant else {
        return Ok(data);
    };
    let rate = data.sample_rate();
    let channels = (0..data.channels())
        .map(|ch| {
            audiostrada_dsp::stretch::stretch_and_shift(data.channel(ch), stretch, pitch, rate)
        })
        .collect();
    Ok(AudioData::new(channels, rate))
}

fn load_job(
    path: &Path,
    original: Option<Arc<AudioData>>,
    rate: u32,
    peaks_dir: Option<&Path>,
) -> Result<LoadOutput, String> {
    let original = match original {
        Some(data) => data,
        None => Arc::new(decode(path)?),
    };
    let cache = peaks_dir.and_then(|dir| peaks_cache(dir, path));
    let peaks = match cache.as_deref().map(Peaks::read) {
        Some(Ok(peaks)) => peaks,
        _ => {
            let peaks = Peaks::compute(&original);
            if let Some(cache) = &cache
                && let Err(e) = peaks.write(cache)
            {
                tracing::warn!("cannot write peak cache {}: {e}", cache.display());
            }
            peaks
        }
    };
    let audio = if original.sample_rate() == rate {
        Arc::clone(&original)
    } else {
        Arc::new(resample(&original, rate).map_err(|e| e.to_string())?)
    };
    Ok((original, audio, Arc::new(peaks)))
}

impl MediaPool {
    pub fn new(rate: u32, peaks_dir: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            rate,
            peaks_dir,
            entries: HashMap::new(),
            tx,
            rx,
            generation: 0,
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Incremented whenever an entry becomes ready or fails.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn set_peaks_dir(&mut self, dir: Option<PathBuf>) {
        self.peaks_dir = dir;
    }

    fn spawn(&self, path: PathBuf, original: Option<Arc<AudioData>>) {
        let tx = self.tx.clone();
        let rate = self.rate;
        let peaks_dir = self.peaks_dir.clone();
        let name = format!(
            "audiostrada-load-{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        let spawned = thread::Builder::new().name(name).spawn(move || {
            let result = load_job(&path, original, rate, peaks_dir.as_deref());
            let _ = tx.send(Loaded { path, rate, result });
        });
        if let Err(e) = spawned {
            tracing::error!("cannot start media loader: {e}");
        }
    }

    /// Starts loading `path` if it is not known yet.
    pub fn request(&mut self, path: &Path) {
        if self.entries.contains_key(path) {
            return;
        }
        self.entries.insert(
            path.to_path_buf(),
            Entry {
                state: MediaState::Loading,
                original: None,
                audio: None,
                peaks: None,
                rate: self.rate,
            },
        );
        self.spawn(path.to_path_buf(), None);
    }

    /// Keeps only the given paths, loading the new ones.
    pub fn retain(&mut self, paths: &[PathBuf]) {
        self.entries.retain(|path, _| paths.contains(path));
        for path in paths {
            self.request(path);
        }
    }

    /// Forgets a path so that the next request reloads it (e.g. after relinking).
    pub fn forget(&mut self, path: &Path) {
        self.entries.remove(path);
    }

    /// Switches the engine rate; ready media is resampled in the background.
    pub fn set_rate(&mut self, rate: u32) {
        // No rate (a device that reports none): resampling to it would fail every file.
        if rate == self.rate || rate == 0 {
            return;
        }
        self.rate = rate;
        let mut jobs = Vec::new();
        for (path, entry) in &mut self.entries {
            if entry.rate != rate && !matches!(entry.state, MediaState::Failed(_)) {
                entry.rate = rate;
                entry.audio = None;
                entry.state = MediaState::Loading;
                jobs.push((path.clone(), entry.original.clone()));
            }
        }
        for (path, original) in jobs {
            self.spawn(path, original);
        }
        self.generation += 1;
    }

    /// Takes finished loads into account; returns true if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(loaded) = self.rx.try_recv() {
            let Some(entry) = self.entries.get_mut(&loaded.path) else {
                continue;
            };
            if loaded.rate != entry.rate {
                continue;
            }
            match loaded.result {
                Ok((original, audio, peaks)) => {
                    entry.original = Some(original);
                    entry.audio = Some(audio);
                    entry.peaks = Some(peaks);
                    entry.state = MediaState::Ready;
                }
                Err(e) => {
                    tracing::warn!("cannot load {}: {e}", loaded.path.display());
                    entry.state = MediaState::Failed(e);
                }
            }
            changed = true;
        }
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// Polls until nothing is loading or `timeout` elapses; returns true if all finished.
    pub fn wait(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            self.poll();
            if self.loading() == 0 {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn loading(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e.state, MediaState::Loading))
            .count()
    }

    pub fn state(&self, path: &Path) -> Option<&MediaState> {
        self.entries.get(path).map(|e| &e.state)
    }

    /// Audio at the engine rate, once loaded.
    pub fn audio(&self, path: &Path) -> Option<&Arc<AudioData>> {
        self.entries.get(path).and_then(|e| e.audio.as_ref())
    }

    /// Decoded audio at its own rate, once loaded.
    pub fn original(&self, path: &Path) -> Option<&Arc<AudioData>> {
        self.entries.get(path).and_then(|e| e.original.as_ref())
    }

    pub fn peaks(&self, path: &Path) -> Option<&Arc<Peaks>> {
        self.entries.get(path).and_then(|e| e.peaks.as_ref())
    }

    /// Paths that could not be loaded, with the reason.
    pub fn failures(&self) -> Vec<(PathBuf, String)> {
        let mut out: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(p, e)| match &e.state {
                MediaState::Failed(reason) => Some((p.clone(), reason.clone())),
                _ => None,
            })
            .collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiostrada_media::write_wav;

    fn temp(name: &str) -> PathBuf {
        let dir = audiostrada_core::scratch::dir(&format!("pool-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn loads_resamples_and_caches_peaks() {
        let dir = temp("load");
        let file = dir.join("tone.wav");
        write_wav(&file, &AudioData::new(vec![vec![0.5; 44_100]], 44_100)).unwrap();
        let mut pool = MediaPool::new(48_000, Some(dir.join("Peaks")));
        pool.request(&file);
        assert!(pool.wait(Duration::from_secs(10)));
        assert!(matches!(pool.state(&file), Some(MediaState::Ready)));
        let audio = pool.audio(&file).unwrap();
        assert_eq!(audio.sample_rate(), 48_000);
        assert!((audio.frames() as i64 - 48_000).abs() <= 2);
        assert_eq!(pool.original(&file).unwrap().frames(), 44_100);
        assert_eq!(std::fs::read_dir(dir.join("Peaks")).unwrap().count(), 1);

        pool.set_rate(96_000);
        assert!(pool.audio(&file).is_none());
        assert!(pool.wait(Duration::from_secs(10)));
        assert_eq!(pool.audio(&file).unwrap().sample_rate(), 96_000);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A device without rate leaves the media as they are: resampled to 0 Hz they would fail.
    #[test]
    fn no_rate_keeps_the_media() {
        let dir = temp("no-rate");
        let file = dir.join("tone.wav");
        write_wav(&file, &AudioData::new(vec![vec![0.5; 4800]], 48_000)).unwrap();
        let mut pool = MediaPool::new(48_000, None);
        pool.request(&file);
        assert!(pool.wait(Duration::from_secs(10)));
        pool.set_rate(0);
        assert!(pool.wait(Duration::from_secs(10)));
        assert!(pool.failures().is_empty());
        assert_eq!(pool.audio(&file).unwrap().sample_rate(), 48_000);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_files_fail() {
        let mut pool = MediaPool::new(48_000, None);
        let missing = PathBuf::from("/nonexistent/audiostrada/missing.wav");
        pool.request(&missing);
        assert!(pool.wait(Duration::from_secs(10)));
        assert_eq!(pool.failures().len(), 1);
        assert!(pool.audio(&missing).is_none());
    }
}
