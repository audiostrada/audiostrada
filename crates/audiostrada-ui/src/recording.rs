//! Recording: the session's health shown while tracks are armed, and what a track records.

use std::path::Path;

use audiostrada_core::routing::Channel;
use audiostrada_core::{Monitoring, RecordSource, Track, TrackId, TrackKind};
use eframe::egui::{self, Color32, RichText, Ui};

use crate::document::Document;
use crate::i18n::tr;
use crate::settings::{ERROR, OK, WARN};
use crate::trf;

/// A recording session, from the moment a track is armed.
#[derive(Debug)]
pub struct Session {
    /// Audio dropouts counted when it started.
    pub xruns: u64,
    /// The health last measured, and when.
    pub health: Option<(std::time::Instant, Health)>,
}

/// The audio buffer, in samples, that reducing the monitoring latency chooses.
pub const SMALL_BUFFER: u32 = 64;

/// Monitoring slower than this (in ms) is shown in orange and can be reduced.
pub const SLOW_MONITORING_MS: f32 = 5.0;

/// The tracks heard live: armed, or always hearing their input.
pub fn live_tracks(doc: &Document) -> Vec<TrackId> {
    doc.project()
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio && (t.armed || t.monitor == Monitoring::Input))
        .map(|t| t.id)
        .collect()
}

/// What the effects of a channel add, those the low latency mode bypasses left out.
pub fn chain_latency(doc: &Document, channel: Channel) -> u32 {
    let player = doc.player();
    let bypassed = player.low_latency_bypassed();
    audiostrada_core::routing::flatten(audiostrada_core::routing::inserts(doc.project(), channel))
        .into_iter()
        .filter(|i| !i.bypass && !bypassed.contains(&i.id))
        .map(|i| player.insert_latency(channel, i.id))
        .sum()
}

fn ms(doc: &Document, samples: u32) -> f32 {
    samples as f32 * 1000.0 / doc.player().engine().sample_rate().max(1) as f32
}

/// The session's health, shown in the transport while tracks are armed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Health {
    /// From the input to the ears, through the slowest chain heard live.
    pub monitoring_ms: f32,
    /// Recording time left on the disk, in seconds.
    pub disk_seconds: Option<u64>,
    pub dropouts: u64,
}

impl Health {
    /// `round_trip`: the interface's measured round trip in samples, or 0 when unknown.
    pub fn measure(
        doc: &Document,
        buffer_ms: f32,
        round_trip: i64,
        dropouts: u64,
        dir: Option<&Path>,
    ) -> Self {
        let master = chain_latency(doc, Channel::Master);
        let slowest = live_tracks(doc)
            .into_iter()
            .map(|t| chain_latency(doc, Channel::Track(t)))
            .max()
            .unwrap_or(0);
        let device = if round_trip > 0 {
            ms(doc, round_trip as u32)
        } else {
            2.0 * buffer_ms
        };
        let channels: usize = doc
            .project()
            .tracks
            .iter()
            .filter(|t| t.armed && t.kind == TrackKind::Audio)
            .map(|t| if t.stereo { 2 } else { 1 })
            .sum::<usize>()
            .max(1);
        let rate = u64::from(doc.player().engine().sample_rate().max(1));
        let disk_seconds = dir
            .and_then(free_space)
            .map(|bytes| bytes / (rate * 4 * channels as u64));
        Self {
            monitoring_ms: device + ms(doc, slowest + master),
            disk_seconds,
            dropouts,
        }
    }
}

/// The health chips; `all` false shows the monitoring one only.
pub fn show_health(ui: &mut Ui, health: &Health, all: bool) {
    let chip = |ui: &mut Ui, text: String, color: Color32, hint: &str| {
        egui::Frame::new()
            .fill(color.gamma_multiply(0.14))
            .corner_radius(9.0)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.label(RichText::new(text).size(11.5).color(color))
            })
            .response
            .on_hover_text(hint);
    };
    let latency = health.monitoring_ms;
    chip(
        ui,
        trf!("Monitoring {ms} ms", ms = format!("{latency:.1}")),
        if latency > 10.0 {
            ERROR
        } else if latency > SLOW_MONITORING_MS {
            WARN
        } else {
            OK
        },
        tr(
            "From the input to the headphones: the audio interface, then the slowest effects \
             heard live. Measure the interface in Preferences, Audio, for an exact figure",
        ),
    );
    if !all {
        return;
    }
    let dropouts = health.dropouts;
    chip(
        ui,
        trf!("{count} dropout(s)", count = dropouts),
        if dropouts == 0 { OK } else { ERROR },
        tr("Audio dropouts since a track was armed"),
    );
    if let Some(seconds) = health.disk_seconds {
        let (h, m) = (seconds / 3600, seconds / 60 % 60);
        let text = if h > 0 {
            trf!("{h} h {m} min free", h = h, m = m)
        } else {
            trf!("{m} min free", m = m)
        };
        let color = match seconds {
            s if s < 600 => ERROR,
            s if s < 3600 => WARN,
            _ => OK,
        };
        chip(
            ui,
            text,
            color,
            tr("Recording time left on the disk, at the armed inputs"),
        );
    }
}

/// What the track records: its input, what its effects make of it, or both.
pub fn record_source(ui: &mut Ui, doc: &mut Document, track: &Track) {
    let choices = [
        (
            RecordSource::Raw,
            tr("Raw"),
            tr("The input as it comes: its effects can still change afterwards"),
        ),
        (
            RecordSource::Processed,
            tr("Processed"),
            tr("What its effects make of the input, on a track under it"),
        ),
        (
            RecordSource::Both,
            tr("Both"),
            tr(
                "The input on the track, and what its effects make of it on a silent track under it",
            ),
        ),
    ];
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (source, name, hint) in choices {
                let on = track.record_source == source;
                if ui.selectable_label(on, name).on_hover_text(hint).clicked() && !on {
                    let id = track.id;
                    doc.history_mut().edit(tr("What to record"), |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.record_source = source;
                        }
                    });
                }
            }
        });
        if track.record_source.processed() && track.monitor == Monitoring::Off {
            ui.label(
                RichText::new(tr(
                    "Its input does not go through its effects (monitoring off): the processed \
                     take would be silent.",
                ))
                .small()
                .color(WARN),
            );
        }
    });
}

/// Bytes free on the disk holding `path`.
#[cfg(unix)]
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let c = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    // SAFETY: a valid C string and a zeroed struct for the call to fill.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    #[allow(
        clippy::useless_conversion,
        reason = "the field types differ between systems"
    )]
    Some(u64::from(stats.f_bavail) * u64::from(stats.f_frsize))
}

/// Bytes free on the disk holding `path`.
#[cfg(windows)]
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let wide: Vec<u16> = existing.as_os_str().encode_wide().chain([0]).collect();
    let mut free = 0u64;
    // SAFETY: a null-terminated wide string, and a u64 for the call to fill.
    unsafe {
        windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            windows::core::PCWSTR(wide.as_ptr()),
            Some(&mut free),
            None,
            None,
        )
    }
    .ok()?;
    Some(free)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_disk_space_is_read() {
        let free = free_space(&std::env::temp_dir());
        assert!(free.is_some_and(|f| f > 0));
    }
}
