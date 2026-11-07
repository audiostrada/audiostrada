//! The recording mode: a layout for a session where musicians play, with the armed inputs,
//! the takes and the session's health. Leaving it puts everything back as it was; it never
//! changes the project.

use std::path::Path;

use audiostrada_core::routing::Channel;
use audiostrada_core::{Monitoring, Track, TrackId, TrackKind};
use eframe::egui::{self, Color32, RichText, Sense, Ui, vec2};

use crate::document::Document;
use crate::i18n::tr;
use crate::settings::{ERROR, OK, WARN};
use crate::theme;
use crate::trf;
use crate::widgets;

/// What the recording mode changed, to put back on leaving.
#[derive(Debug)]
pub struct Saved {
    pub layout: Option<String>,
    /// The buffer size before, when the mode changed it.
    pub buffer: Option<u32>,
    pub low_latency: bool,
    /// Audio dropouts counted when the mode started.
    pub xruns: u64,
    /// The health last measured, and when.
    pub health: Option<(std::time::Instant, Health)>,
}

/// A chain heard live adding more than this is shown in orange.
const SLOW_MS: f32 = 2.0;

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

/// The session's health, shown in the transport during the recording mode.
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

/// The health chips.
pub fn show_health(ui: &mut Ui, health: &Health) {
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
        } else if latency > 5.0 {
            WARN
        } else {
            OK
        },
        tr(
            "From the input to the headphones: the audio interface, then the slowest effects \
             heard live. Measure the interface in Preferences, Audio, for an exact figure",
        ),
    );
    let dropouts = health.dropouts;
    chip(
        ui,
        trf!("{count} dropout(s)", count = dropouts),
        if dropouts == 0 { OK } else { ERROR },
        tr("Audio dropouts since the recording mode started"),
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

/// The armed inputs panel: the session's health, a card per track heard live, then a way to
/// arm another one.
pub fn inputs_pane(ui: &mut Ui, doc: &mut Document, health: Option<&Health>) {
    doc.update_levels();
    let live = live_tracks(doc);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            if let Some(health) = health {
                ui.horizontal_wrapped(|ui| show_health(ui, health));
                ui.add_space(6.0);
            }
            ui.label(
                RichText::new(trf!("Armed tracks · {count}", count = live.len()))
                    .strong()
                    .weak(),
            );
            ui.add_space(4.0);
            for id in &live {
                if let Some(track) = doc.project().track(*id).cloned() {
                    input_card(ui, doc, &track);
                    ui.add_space(6.0);
                }
            }
            if live.is_empty() {
                ui.label(
                    RichText::new(tr(
                        "No track is armed. Arm one to hear its input and record it.",
                    ))
                    .weak(),
                );
            }
            ui.add_space(4.0);
            let others: Vec<(TrackId, String)> = doc
                .project()
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Audio && !live.contains(&t.id))
                .map(|t| (t.id, t.name.clone()))
                .collect();
            if !others.is_empty() {
                ui.menu_button(tr("Arm a track"), |ui| {
                    for (id, name) in others {
                        if ui.button(name).clicked() {
                            set_armed(doc, id, true);
                            ui.close();
                        }
                    }
                });
            }
        });
}

fn set_armed(doc: &mut Document, id: TrackId, armed: bool) {
    use audiostrada_core::groups::{self, Link};
    doc.history_mut()
        .modify_unrecorded(|p| groups::set_switch(p, id, Link::Arm, armed));
}

fn input_card(ui: &mut Ui, doc: &mut Document, track: &Track) {
    let p = theme::current(ui);
    let color = Color32::from_rgb(track.color[0], track.color[1], track.color[2]);
    let card = egui::Frame::new()
        .fill(p.surface)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(6.0)
        .inner_margin(egui::Margin {
            left: 12,
            right: 8,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if theme::arm_toggle(ui, vec2(22.0, 20.0), track.armed)
                    .on_hover_text(tr("Record arm"))
                    .clicked()
                {
                    set_armed(doc, track.id, !track.armed);
                }
                ui.add(egui::Label::new(RichText::new(&track.name).strong()).truncate());
            });
            ui.horizontal(|ui| {
                ui.menu_button(crate::mixer::input_label(track.input, track.stereo), |ui| {
                    crate::mixer::input_choices(ui, doc, track)
                });
                let (rect, _) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::hover());
                crate::mixer::monitor_toggle(ui, rect, doc, track);
            });
            let level = doc.track_level(track.id);
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
            let db = level.level[0].max(level.level[1]);
            let lit = egui::Rect::from_min_max(
                bar.min,
                egui::pos2(
                    bar.left() + bar.width() * widgets::meter_fraction(db),
                    bar.bottom(),
                ),
            );
            widgets::paint_meter_bar(ui.painter(), bar, lit);
            let hold = level.hold[0].max(level.hold[1]);
            let x = bar.left() + bar.width() * widgets::meter_fraction(hold);
            ui.painter()
                .vline(x, bar.y_range(), egui::Stroke::new(1.5, p.text));
            ui.horizontal(|ui| {
                if level.clipped {
                    ui.label(RichText::new(tr("Clipped")).small().color(ERROR));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{hold:.1} dB")).small().weak());
                });
            });
            chain(ui, doc, track);
        });
    let rect = card.response.rect;
    ui.painter().vline(
        rect.left() + 3.0,
        rect.y_range().shrink(5.0),
        egui::Stroke::new(3.0, color),
    );
}

/// The track's effects as chips, and what they add.
fn chain(ui: &mut Ui, doc: &Document, track: &Track) {
    let channel = Channel::Track(track.id);
    let bypassed = doc.player().low_latency_bypassed().to_vec();
    let inserts = audiostrada_core::routing::flatten(&track.inserts);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(tr("Heard through")).small().weak());
        if inserts.is_empty() {
            ui.label(RichText::new(tr("no effect")).small().weak());
        }
        for insert in inserts {
            let name = crate::mixer::effect_name(doc, &insert.effect);
            let off = insert.bypass || bypassed.contains(&insert.id);
            let text = RichText::new(name).small();
            ui.label(if off {
                text.strikethrough().weak()
            } else {
                text
            });
        }
        let added = ms(doc, chain_latency(doc, channel));
        let color = if added > SLOW_MS { WARN } else { OK };
        ui.label(
            RichText::new(trf!("+{ms} ms", ms = format!("{added:.1}")))
                .small()
                .color(color),
        )
        .on_hover_text(tr("What the track's effects add to what is heard"));
    });
}

/// The takes panel: the takes of the armed tracks, then the modules' tabs.
pub fn takes_pane(
    ui: &mut Ui,
    doc: &mut Document,
    extensions: &mut [Box<dyn crate::extension::Extension>],
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let tracks: Vec<Track> = doc
                .project()
                .tracks
                .iter()
                .filter(|t| t.armed && t.kind == TrackKind::Audio)
                .map(|t| (**t).clone())
                .collect();
            if tracks.is_empty() {
                ui.label(RichText::new(tr("The takes of the armed tracks appear here.")).weak());
            }
            for track in tracks {
                ui.label(RichText::new(&track.name).strong());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").color(theme::RECORD));
                    ui.label(&track.playlist);
                });
                let mut chosen = None;
                for (i, take) in track.alternates.iter().enumerate().rev() {
                    if ui
                        .selectable_label(false, &take.name)
                        .on_hover_text(tr("Play this take instead"))
                        .clicked()
                    {
                        chosen = Some(i);
                    }
                }
                if let Some(i) = chosen {
                    doc.history_mut().edit(tr("Choose take"), |p| {
                        audiostrada_core::edit::select_playlist(p, track.id, i)
                    });
                }
                ui.add_space(8.0);
            }
            for extension in extensions.iter_mut() {
                for (i, title) in extension.recording_tabs().into_iter().enumerate() {
                    egui::CollapsingHeader::new(title)
                        .id_salt((extension.name(), i))
                        .default_open(true)
                        .show(ui, |ui| extension.show_recording_tab(i, ui, doc));
                }
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
