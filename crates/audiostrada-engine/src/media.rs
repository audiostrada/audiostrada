//! Background loading of media files: decoding, peak caches and resampling to the engine rate.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, mpsc};
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
    /// Dropped when unread for [`KEEP_ORIGINAL`] if it is not `audio` itself, decoded again
    /// when asked for.
    original: Option<Arc<AudioData>>,
    original_read: Instant,
    format: Option<(usize, u32, usize)>,
    /// `audio` locked in RAM, as the audio thread reads it (dropped before it).
    resident: Option<audiostrada_rt::Resident>,
    audio: Option<Arc<AudioData>>,
    peaks: Option<Arc<Peaks>>,
    /// Engine rate the pending or current `audio` was made for.
    rate: u32,
}

/// How long an original at another rate than the engine's stays in memory once unread.
const KEEP_ORIGINAL: Duration = Duration::from_secs(30);

/// Decoded original, audio at the engine rate, and peaks.
type LoadOutput = (Arc<AudioData>, Arc<AudioData>, Arc<Peaks>);

struct Loaded {
    path: PathBuf,
    rate: u32,
    result: Result<LoadOutput, String>,
}

/// Files loaded at once: each takes a few threads and its whole audio in memory, so a large
/// project would otherwise decode everything together.
fn loading_slot() -> impl Drop {
    static SLOTS: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());
    struct Slot;
    impl Drop for Slot {
        fn drop(&mut self) {
            *SLOTS.0.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
            SLOTS.1.notify_one();
        }
    }
    let most = thread::available_parallelism().map_or(2, |n| (n.get() / 3).clamp(2, 6));
    let mut busy = SLOTS.0.lock().unwrap_or_else(|e| e.into_inner());
    while *busy >= most {
        busy = SLOTS.1.wait(busy).unwrap_or_else(|e| e.into_inner());
    }
    *busy += 1;
    Slot
}

/// Decoded media keyed by absolute path, at the engine sample rate.
pub struct MediaPool {
    rate: u32,
    peaks_dir: Option<PathBuf>,
    entries: HashMap<PathBuf, Entry>,
    tx: mpsc::Sender<Loaded>,
    rx: mpsc::Receiver<Loaded>,
    generation: u64,
    /// Originals asked for since the last poll.
    asked: RefCell<HashSet<PathBuf>>,
    keep_original: Duration,
    reloading: HashSet<PathBuf>,
    reloaded_tx: mpsc::Sender<(PathBuf, Result<Arc<AudioData>, String>)>,
    reloaded_rx: mpsc::Receiver<(PathBuf, Result<Arc<AudioData>, String>)>,
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

/// Decodes a file, edited, stretched and transposed when `path` is a variant.
/// `file`, the plain file a variant stands for, may be given already decoded.
fn decode(path: &Path, file_data: Option<Arc<AudioData>>) -> Result<AudioData, String> {
    let (file, variant) = audiostrada_core::parse_variant(path);
    let mut data = match file_data {
        Some(data) => (*data).clone(),
        None => load(&file).map_err(|e| e.to_string())?,
    };
    if let Some(edits) = audiostrada_core::audio_edit::edits_of(path)? {
        data = crate::retouch::apply(&data, &edits);
    }
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
    (original, file_data): (Option<Arc<AudioData>>, Option<Arc<AudioData>>),
    rate: u32,
    peaks_dir: Option<&Path>,
) -> Result<LoadOutput, String> {
    let original = match original {
        Some(data) => data,
        None => Arc::new(decode(path, file_data)?),
    };
    // Edits change often while they are made: their peaks are not kept.
    let edited = audiostrada_core::audio_edit::strip_edits(path) != path;
    let cache = peaks_dir
        .filter(|_| !edited)
        .and_then(|dir| peaks_cache(dir, path));
    // The peaks and the conversion to the project's rate are made side by side.
    let (peaks, audio) = std::thread::scope(|scope| {
        let audio = (original.sample_rate() != rate).then(|| {
            let original = &original;
            scope.spawn(move || resample(original, rate).map_err(|e| e.to_string()))
        });
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
        let audio = match audio {
            Some(job) => Arc::new(
                job.join()
                    .unwrap_or_else(|_| Err("resampling panicked".into()))?,
            ),
            None => Arc::clone(&original),
        };
        Ok::<_, String>((peaks, audio))
    })?;
    Ok((original, audio, Arc::new(peaks)))
}

impl MediaPool {
    pub fn new(rate: u32, peaks_dir: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (reloaded_tx, reloaded_rx) = mpsc::channel();
        Self {
            rate,
            peaks_dir,
            entries: HashMap::new(),
            tx,
            rx,
            generation: 0,
            asked: RefCell::default(),
            keep_original: KEEP_ORIGINAL,
            reloading: HashSet::new(),
            reloaded_tx,
            reloaded_rx,
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
        // A variant of a file already decoded starts from it.
        let (file, _) = audiostrada_core::parse_variant(&path);
        let file_data = (original.is_none() && file != path)
            .then(|| self.entries.get(&file).and_then(|e| e.original.clone()))
            .flatten();
        let tx = self.tx.clone();
        let rate = self.rate;
        let peaks_dir = self.peaks_dir.clone();
        let name = format!(
            "audiostrada-load-{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        let spawned = thread::Builder::new().name(name).spawn(move || {
            let _slot = loading_slot();
            let result = load_job(&path, (original, file_data), rate, peaks_dir.as_deref());
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
                original_read: Instant::now(),
                format: None,
                resident: None,
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
                entry.resident = None;
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
                    entry.format = Some((
                        original.channels(),
                        original.sample_rate(),
                        original.frames(),
                    ));
                    entry.original = Some(original);
                    entry.original_read = Instant::now();
                    entry.resident = None;
                    let channels: Vec<&[f32]> =
                        (0..audio.channels()).map(|ch| audio.channel(ch)).collect();
                    entry.resident = audiostrada_rt::keep_resident(&channels);
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
        self.keep_originals();
        changed
    }

    /// Decodes again the originals asked for, drops those unread for a while.
    fn keep_originals(&mut self) {
        while let Ok((path, result)) = self.reloaded_rx.try_recv() {
            self.reloading.remove(&path);
            match (self.entries.get_mut(&path), result) {
                (Some(entry), Ok(data)) if entry.original.is_none() => {
                    entry.original = Some(data);
                    entry.original_read = Instant::now();
                }
                (_, Err(e)) => tracing::warn!("cannot decode {} again: {e}", path.display()),
                _ => {}
            }
        }
        let now = Instant::now();
        for path in self.asked.take() {
            let Some(entry) = self.entries.get_mut(&path) else {
                continue;
            };
            entry.original_read = now;
            if entry.original.is_none()
                && matches!(entry.state, MediaState::Ready)
                && self.reloading.insert(path.clone())
            {
                let (file, _) = audiostrada_core::parse_variant(&path);
                let file_data = (file != path)
                    .then(|| self.entries.get(&file).and_then(|e| e.original.clone()))
                    .flatten();
                let tx = self.reloaded_tx.clone();
                let spawned = thread::Builder::new()
                    .name("audiostrada-reload".into())
                    .spawn(move || {
                        let _slot = loading_slot();
                        let result = decode(&path, file_data).map(Arc::new);
                        let _ = tx.send((path, result));
                    });
                if let Err(e) = spawned {
                    tracing::error!("cannot start media loader: {e}");
                }
            }
        }
        for entry in self.entries.values_mut() {
            let shared = match (&entry.original, &entry.audio) {
                (Some(original), Some(audio)) => Arc::ptr_eq(original, audio),
                _ => true,
            };
            if !shared && now.duration_since(entry.original_read) > self.keep_original {
                entry.original = None;
            }
        }
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

    /// Files loading, originals decoded again included.
    pub fn loading(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e.state, MediaState::Loading))
            .count()
            + self.reloading.len()
            + self
                .asked
                .borrow()
                .iter()
                .filter(|p| self.reloads(p))
                .count()
    }

    pub fn state(&self, path: &Path) -> Option<&MediaState> {
        self.entries.get(path).map(|e| &e.state)
    }

    /// Audio at the engine rate, once loaded.
    pub fn audio(&self, path: &Path) -> Option<&Arc<AudioData>> {
        self.entries.get(path).and_then(|e| e.audio.as_ref())
    }

    /// Decoded audio at its own rate, once loaded. At another rate than the engine's it is
    /// dropped when unread for a while: asking for it decodes it again, `None` meanwhile.
    pub fn original(&self, path: &Path) -> Option<&Arc<AudioData>> {
        let entry = self.entries.get(path)?;
        let mut asked = self.asked.borrow_mut();
        if !asked.contains(path) {
            asked.insert(path.to_path_buf());
        }
        entry.original.as_ref()
    }

    /// Whether asking for the original of `path` decodes it again.
    fn reloads(&self, path: &Path) -> bool {
        self.entries
            .get(path)
            .is_some_and(|e| e.original.is_none() && matches!(e.state, MediaState::Ready))
            && !self.reloading.contains(path)
    }

    /// Channels, rate and frames of the decoded file, once loaded.
    pub fn format(&self, path: &Path) -> Option<(usize, u32, usize)> {
        self.entries.get(path)?.format
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
    fn an_original_at_another_rate_is_dropped_unread_and_decoded_again_when_asked() {
        let dir = temp("reload");
        let other = dir.join("other.wav");
        let same = dir.join("same.wav");
        write_wav(&other, &AudioData::new(vec![vec![0.5; 44_100]], 44_100)).unwrap();
        write_wav(&same, &AudioData::new(vec![vec![0.5; 48_000]], 48_000)).unwrap();
        let mut pool = MediaPool::new(48_000, None);
        pool.keep_original = Duration::from_millis(50);
        pool.request(&other);
        pool.request(&same);
        assert!(pool.wait(Duration::from_secs(10)));
        thread::sleep(Duration::from_millis(100));
        pool.poll();
        assert!(pool.original(&other).is_none());
        assert!(pool.original(&same).is_some());
        assert_eq!(pool.format(&other), Some((1, 44_100, 44_100)));
        assert!(pool.audio(&other).is_some());
        assert!(pool.wait(Duration::from_secs(10)));
        assert_eq!(pool.original(&other).unwrap().frames(), 44_100);
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
