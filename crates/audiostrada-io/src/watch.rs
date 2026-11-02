//! Device list watcher for backends without change notifications: polls a cheap fingerprint of
//! the present devices on its own thread and counts the changes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

const INTERVAL: Duration = Duration::from_secs(2);
const STEP: Duration = Duration::from_millis(100);

pub(crate) struct DeviceWatcher {
    generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for DeviceWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceWatcher")
            .field("generation", &self.generation())
            .finish_non_exhaustive()
    }
}

impl DeviceWatcher {
    pub fn start(fingerprint: impl Fn() -> Vec<String> + Send + 'static) -> Self {
        Self::with_interval(fingerprint, INTERVAL)
    }

    fn with_interval(
        fingerprint: impl Fn() -> Vec<String> + Send + 'static,
        interval: Duration,
    ) -> Self {
        let generation = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (g, s) = (Arc::clone(&generation), Arc::clone(&stop));
        let thread = std::thread::Builder::new()
            .name("device-watch".into())
            .spawn(move || {
                let mut last = fingerprint();
                while !s.load(Ordering::Relaxed) {
                    let mut waited = Duration::ZERO;
                    while waited < interval && !s.load(Ordering::Relaxed) {
                        std::thread::sleep(STEP.min(interval));
                        waited += STEP.min(interval);
                    }
                    let now = fingerprint();
                    if now != last {
                        last = now;
                        g.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
            .ok();
        Self {
            generation,
            stop,
            thread,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn counts_changes_of_the_fingerprint() {
        let devices = Arc::new(Mutex::new(vec!["a".to_owned()]));
        let d = Arc::clone(&devices);
        let watcher = DeviceWatcher::with_interval(
            move || d.lock().unwrap().clone(),
            Duration::from_millis(10),
        );
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(watcher.generation(), 0);
        devices.lock().unwrap().push("b".into());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while watcher.generation() == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(watcher.generation(), 1);
    }
}
