use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;

use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

/// Log filter, using the `tracing` directive syntax (e.g. `AUDIOSTRADA_LOG=debug,audiostrada::rt=trace`).
const LOG_ENV: &str = "AUDIOSTRADA_LOG";

/// The window's log, the previous session's kept beside it as `audiostrada.log.1`.
pub fn path() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::cache_dir)
        .map(|d| d.join("audiostrada").join("audiostrada.log"))
}

/// Logs to the standard error, and to the log file too when `to_file` (the window).
pub fn init(to_file: bool) {
    // The clipboard warns when it falls back from Wayland to X11, which works as well.
    let filter = EnvFilter::try_from_env(LOG_ENV)
        .unwrap_or_else(|_| EnvFilter::new("info,arboard::platform::linux=error"));
    let file = to_file.then(open).flatten().map(|(file, path)| {
        let layer = fmt::layer()
            .with_thread_names(true)
            .with_ansi(false)
            .with_writer(Mutex::new(file));
        (layer, path)
    });
    let (file_layer, path) = file.unzip();
    tracing_subscriber::registry()
        .with(filter)
        // Standard output is for results (and the MCP protocol).
        .with(
            fmt::layer()
                .with_thread_names(true)
                .with_writer(std::io::stderr),
        )
        .with(file_layer)
        .init();
    if let Some(path) = path {
        tracing::info!("log file: {}", path.display());
    }
}

fn open() -> Option<(File, PathBuf)> {
    let path = path()?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    let _ = std::fs::rename(&path, path.with_extension("log.1"));
    Some((File::create(&path).ok()?, path))
}
