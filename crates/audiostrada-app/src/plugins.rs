//! Plugin discovery commands.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use audiostrada_plugins::Format;
use audiostrada_plugins::scan::{self, Database};

/// Where the plugin list is cached between runs.
pub fn database_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("audiostrada").join("plugins.json"))
}

/// Child-process side of a scan: prints the plugins of one file as JSON.
pub fn scan_one(format: &str, path: &Path) -> anyhow::Result<()> {
    let format =
        Format::from_prefix(format).ok_or_else(|| anyhow!("unknown plugin format '{format}'"))?;
    let plugins = audiostrada_plugins::scan_file(format, path).map_err(|e| anyhow!(e))?;
    println!("{}", serde_json::to_string(&plugins)?);
    Ok(())
}

pub fn list(rescan: bool) -> anyhow::Result<()> {
    let cache = database_path().context("no cache folder on this system")?;
    let extra = crate::prefs::Prefs::load().ui.plugin_dirs;
    let exe =
        audiostrada_plugins::own_executable().context("cannot find the audiostrada executable")?;
    println!("Scanning plugin folders…");
    let database: Database =
        scan::scan_in_background(cache, extra, exe, rescan, Default::default())
            .join()
            .map_err(|_| anyhow!("the scan crashed"))?;
    let mut plugins: Vec<_> = database.plugins().collect();
    plugins.sort_by_key(|p| {
        (
            p.format.name(),
            p.vendor.to_lowercase(),
            p.name.to_lowercase(),
        )
    });
    for p in &plugins {
        let kind = if p.instrument { "instrument" } else { "effect" };
        println!(
            "{:5} {:<32} {:<24} {:<10} {}",
            p.format.name(),
            p.name,
            p.vendor,
            kind,
            p.path.display()
        );
    }
    let failures: Vec<_> = database.failures().collect();
    println!(
        "\n{} plugin(s), {} file(s) failed",
        plugins.len(),
        failures.len()
    );
    for (path, reason) in failures {
        println!("  {}: {reason}", path.display());
    }
    Ok(())
}
