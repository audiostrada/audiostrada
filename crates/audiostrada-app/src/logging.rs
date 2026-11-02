use tracing_subscriber::EnvFilter;

/// Log filter, using the `tracing` directive syntax (e.g. `AUDIOSTRADA_LOG=debug,audiostrada::rt=trace`).
const LOG_ENV: &str = "AUDIOSTRADA_LOG";

pub fn init() {
    // The clipboard warns when it falls back from Wayland to X11, which works as well.
    let filter = EnvFilter::try_from_env(LOG_ENV)
        .unwrap_or_else(|_| EnvFilter::new("info,arboard::platform::linux=error"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_thread_names(true)
        // Standard output is for results (and the MCP protocol).
        .with_writer(std::io::stderr)
        .init();
}
