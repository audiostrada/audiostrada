//! Plugin discovery: standard folders per format, a database cached on disk, and scanning in a
//! child process so that a crashing plugin cannot take the application down.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{Format, PluginDescriptor};

/// Subcommand of the application binary that scans one file and prints JSON.
pub const SCAN_COMMAND: &str = "plugin-scan";

fn env_dirs(var: &str) -> Vec<PathBuf> {
    std::env::var_os(var)
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default()
}

/// Standard plugin folders of `format` on this system.
pub fn default_dirs(format: Format) -> Vec<PathBuf> {
    let home = dirs_home();
    let mut out = Vec::new();
    match format {
        Format::Clap => {
            out.extend(env_dirs("CLAP_PATH"));
            if cfg!(target_os = "windows") {
                out.extend(
                    std::env::var_os("COMMONPROGRAMFILES").map(|d| PathBuf::from(d).join("CLAP")),
                );
                out.extend(
                    std::env::var_os("LOCALAPPDATA")
                        .map(|d| PathBuf::from(d).join("Programs/Common/CLAP")),
                );
            } else if cfg!(target_os = "macos") {
                out.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
                out.extend(home.map(|h| h.join("Library/Audio/Plug-Ins/CLAP")));
            } else {
                out.extend(home.map(|h| h.join(".clap")));
                out.push(PathBuf::from("/usr/lib/clap"));
                out.push(PathBuf::from("/usr/local/lib/clap"));
            }
        }
        Format::Vst3 => {
            if cfg!(target_os = "windows") {
                out.extend(
                    std::env::var_os("COMMONPROGRAMFILES").map(|d| PathBuf::from(d).join("VST3")),
                );
            } else if cfg!(target_os = "macos") {
                out.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
                out.extend(home.map(|h| h.join("Library/Audio/Plug-Ins/VST3")));
            } else {
                out.extend(home.map(|h| h.join(".vst3")));
                out.push(PathBuf::from("/usr/lib/vst3"));
                out.push(PathBuf::from("/usr/local/lib/vst3"));
            }
        }
        Format::Lv2 => {
            out.extend(env_dirs("LV2_PATH"));
            if cfg!(target_os = "macos") {
                out.push(PathBuf::from("/Library/Audio/Plug-Ins/LV2"));
                out.extend(home.map(|h| h.join("Library/Audio/Plug-Ins/LV2")));
            } else if !cfg!(target_os = "windows") {
                out.extend(home.map(|h| h.join(".lv2")));
                out.push(PathBuf::from("/usr/lib/lv2"));
                out.push(PathBuf::from("/usr/local/lib/lv2"));
            }
        }
        Format::Au => {
            if cfg!(target_os = "macos") {
                out.push(PathBuf::from("/Library/Audio/Plug-Ins/Components"));
                out.extend(home.map(|h| h.join("Library/Audio/Plug-Ins/Components")));
            }
        }
    }
    out.dedup();
    out
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn extension(format: Format) -> &'static str {
    match format {
        Format::Clap => "clap",
        Format::Vst3 => "vst3",
        Format::Lv2 => "lv2",
        Format::Au => "component",
    }
}

/// Plugin files and bundles of `format` under `dirs`, recursively.
pub fn find_files(format: Format, dirs: &[PathBuf]) -> Vec<PathBuf> {
    let ext = extension(format);
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
            {
                out.push(path);
            } else if path.is_dir() && depth < 6 {
                stack.push((path, depth + 1));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct FileEntry {
    format: Format,
    modified: u64,
    result: Result<Vec<PluginDescriptor>, String>,
}

/// Raised when scans learn more about plugins, so that older caches are scanned again.
const DATABASE_VERSION: u32 = 1;

/// Every plugin found, cached between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Database {
    #[serde(default)]
    version: u32,
    files: BTreeMap<PathBuf, FileEntry>,
}

impl Database {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|d| d.version == DATABASE_VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    pub fn plugins(&self) -> impl Iterator<Item = &PluginDescriptor> {
        self.files
            .values()
            .filter_map(|f| f.result.as_ref().ok())
            .flatten()
    }

    /// Files that could not be scanned, with the reason.
    pub fn failures(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.files
            .iter()
            .filter_map(|(p, f)| f.result.as_ref().err().map(|e| (p.as_path(), e.as_str())))
    }

    /// Finds a plugin by its project identifier (`clap:...`).
    pub fn find(&self, key: &str) -> Option<&PluginDescriptor> {
        self.plugins().find(|p| p.key() == key)
    }

    /// Files to scan: new or modified since the last scan. Removed files are forgotten.
    pub fn pending(&mut self, extra_dirs: &[PathBuf]) -> Vec<(Format, PathBuf)> {
        let mut seen = Vec::new();
        let mut pending = Vec::new();
        for format in Format::ALL {
            let mut dirs = default_dirs(format);
            dirs.extend(extra_dirs.iter().cloned());
            for path in find_files(format, &dirs) {
                let stamp = modified(&path);
                if self
                    .files
                    .get(&path)
                    .is_none_or(|f| f.modified != stamp || f.format != format)
                {
                    pending.push((format, path.clone()));
                }
                seen.push(path);
            }
        }
        self.files.retain(|p, _| seen.contains(p));
        pending
    }

    pub fn record(
        &mut self,
        format: Format,
        path: PathBuf,
        result: Result<Vec<PluginDescriptor>, String>,
    ) {
        let modified = modified(&path);
        self.version = DATABASE_VERSION;
        self.files.insert(
            path,
            FileEntry {
                format,
                modified,
                result,
            },
        );
    }

    /// Scans everything pending with `scan`; returns the number of files scanned.
    pub fn refresh(
        &mut self,
        extra_dirs: &[PathBuf],
        mut scan: impl FnMut(Format, &Path) -> Result<Vec<PluginDescriptor>, String>,
    ) -> usize {
        let pending = self.pending(extra_dirs);
        for (format, path) in &pending {
            let result = scan(*format, path);
            self.record(*format, path.clone(), result);
        }
        pending.len()
    }
}

/// Loads the cached database (or starts afresh when `full`), scans what changed in child
/// processes of `exe`, and saves the result to `cache`.
pub fn scan_in_background(
    cache: PathBuf,
    extra_dirs: Vec<PathBuf>,
    exe: PathBuf,
    full: bool,
) -> std::thread::JoinHandle<Database> {
    std::thread::spawn(move || {
        let mut database = if full {
            Database::default()
        } else {
            Database::load(&cache)
        };
        let scanned = database.refresh(&extra_dirs, |format, path| {
            scan_out_of_process(&exe, format, path, Duration::from_secs(30))
        });
        if (scanned > 0 || full)
            && let Err(e) = database.save(&cache)
        {
            tracing::warn!("cannot save the plugin list to {}: {e}", cache.display());
        }
        database
    })
}

/// Runs `exe plugin-scan FORMAT PATH` and parses its output; crashes and hangs become errors.
pub fn scan_out_of_process(
    exe: &Path,
    format: Format,
    path: &Path,
    timeout: Duration,
) -> Result<Vec<PluginDescriptor>, String> {
    let mut child = Command::new(exe)
        .arg(SCAN_COMMAND)
        .arg(format.prefix())
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start the scanner: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no scanner output")?;
    let mut stderr = child.stderr.take().ok_or("no scanner output")?;
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("scanning took more than {} s", timeout.as_secs()));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if !status.success() {
        let reason = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned();
        return Err(if reason.is_empty() {
            format!("the plugin crashed the scanner ({status})")
        } else {
            reason
        });
    }
    serde_json::from_str(&out).map_err(|e| format!("invalid scanner output: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn scanner_failures_become_errors() {
        let path = Path::new("x.clap");
        let timeout = Duration::from_secs(5);
        let crashed = scan_out_of_process(Path::new("false"), Format::Clap, path, timeout);
        assert!(crashed.unwrap_err().contains("crashed the scanner"));
        let garbage = scan_out_of_process(Path::new("echo"), Format::Clap, path, timeout);
        assert!(garbage.unwrap_err().contains("invalid scanner output"));
        let missing = scan_out_of_process(Path::new("/nonexistent"), Format::Clap, path, timeout);
        assert!(missing.unwrap_err().contains("cannot start"));
    }

    #[test]
    fn every_format_has_folders_and_an_extension() {
        for format in [Format::Clap, Format::Vst3] {
            assert!(!default_dirs(format).is_empty());
        }
        assert_eq!(extension(Format::Au), "component");
        assert!(find_files(Format::Lv2, &[PathBuf::from("/nonexistent")]).is_empty());
    }

    #[test]
    fn database_tracks_files_and_failures() {
        let dir = std::env::temp_dir().join(format!("audiostrada-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("vendor")).unwrap();
        std::fs::write(dir.join("vendor/good.clap"), b"").unwrap();
        std::fs::write(dir.join("bad.clap"), b"").unwrap();
        std::fs::write(dir.join("notes.txt"), b"").unwrap();
        let mut db = Database::default();
        let scanned = db.refresh(std::slice::from_ref(&dir), |format, path| {
            if path.ends_with("bad.clap") {
                return Err("broken".into());
            }
            Ok(vec![PluginDescriptor {
                format,
                id: "org.example.good".into(),
                name: "Good".into(),
                vendor: "Example".into(),
                version: "1".into(),
                path: path.to_path_buf(),
                instrument: false,
                tags: Vec::new(),
            }])
        });
        assert!(scanned >= 2);
        assert!(db.find("clap:org.example.good").is_some());
        assert_eq!(
            db.failures()
                .filter(|(p, _)| p.ends_with("bad.clap"))
                .count(),
            1
        );
        assert_eq!(
            db.refresh(std::slice::from_ref(&dir), |_, _| unreachable!()),
            0,
            "cached"
        );
        let file = dir.join("db.json");
        db.save(&file).unwrap();
        assert_eq!(Database::load(&file), db);
        std::fs::remove_file(dir.join("bad.clap")).unwrap();
        db.pending(std::slice::from_ref(&dir));
        assert_eq!(
            db.failures()
                .filter(|(p, _)| p.ends_with("bad.clap"))
                .count(),
            0
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
