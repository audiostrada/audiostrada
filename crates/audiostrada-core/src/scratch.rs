//! Temporary folders, all under `audiostrada/<process id>` in the system's temporary folder.
//! What a finished, crashed or killed process left there is removed by the next one.

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;

fn root() -> PathBuf {
    std::env::temp_dir().join("audiostrada")
}

/// This process's folder, created, the folders of processes gone swept the first time.
pub fn process_dir() -> PathBuf {
    static SWEPT: Once = Once::new();
    SWEPT.call_once(|| sweep(&root()));
    let dir = root().join(std::process::id().to_string());
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// A path named `name` in this process's folder; the folder exists, the path itself is left
/// to the caller.
pub fn dir(name: &str) -> PathBuf {
    process_dir().join(name)
}

fn sweep(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let me = std::process::id();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid != me && gone(pid, &entry.path()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// The process ended: no longer listed on Linux, untouched for two days elsewhere.
fn gone(pid: u32, dir: &Path) -> bool {
    if cfg!(target_os = "linux") {
        return !Path::new(&format!("/proc/{pid}")).exists();
    }
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > Duration::from_secs(2 * 24 * 3600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_of_ended_processes_are_swept() {
        let root = std::env::temp_dir().join(format!("audiostrada-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let alive = root.join(std::process::id().to_string());
        let other = root.join("not-a-process");
        for d in [&alive, &other] {
            std::fs::create_dir_all(d).unwrap();
        }
        let dead = root.join(u32::MAX.to_string());
        std::fs::create_dir_all(&dead).unwrap();
        sweep(&root);
        assert!(alive.exists() && other.exists());
        if cfg!(target_os = "linux") {
            assert!(!dead.exists());
        }
        std::fs::remove_dir_all(&root).unwrap();
        assert!(dir("x").parent().is_some_and(Path::exists));
    }
}
