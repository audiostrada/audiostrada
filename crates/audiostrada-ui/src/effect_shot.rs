//! Development aid: with `AUDIOSTRADA_SCREENSHOT_EFFECT=<file.ppm>`, opens the editor of the
//! built-in effect `AUDIOSTRADA_SCREENSHOT_ID` as soon as one is added (through `--control`) to
//! the track `AUDIOSTRADA_SCREENSHOT_TRACK` (`master` for the master; any track if unset), drawn
//! inside the main window, saves the editor a few seconds later and quits.

use std::cell::Cell;

use audiostrada_core::routing::Channel;
use audiostrada_core::{Insert, InsertId, Project};
use eframe::egui::{self, Rect, ViewportId};

/// Frames between the opening and the picture: the editor settles, meters move.
const SETTLE: u64 = 240;

thread_local! {
    static TARGET: Cell<Option<(ViewportId, u64)>> = const { Cell::new(None) };
    static BASELINE: Cell<Option<u64>> = const { Cell::new(None) };
}

fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("AUDIOSTRADA_SCREENSHOT_EFFECT").map(Into::into)
}

/// The insert whose editor to open, once.
pub(crate) fn open(ctx: &egui::Context, project: &Project) -> Option<(Channel, InsertId)> {
    path()?;
    ctx.set_embed_viewports(true);
    ctx.request_repaint();
    if TARGET.get().is_some() {
        return None;
    }
    let effect = std::env::var("AUDIOSTRADA_SCREENSHOT_ID").unwrap_or_default();
    let track = std::env::var("AUDIOSTRADA_SCREENSHOT_TRACK").ok();
    // Only an effect added after the start: the ones already there are left alone.
    let baseline = BASELINE.get().unwrap_or_else(|| {
        let tracks = project.tracks.iter().flat_map(|t| &t.inserts);
        let max = tracks.chain(&project.master.inserts).map(|i| i.id.0).max();
        max.unwrap_or(0)
    });
    BASELINE.set(Some(baseline));
    let new = |i: &&Insert| i.effect == effect && i.id.0 > baseline;
    let found = project
        .tracks
        .iter()
        .filter(|t| track.as_deref().is_none_or(|name| t.name == name))
        .find_map(|t| Some((Channel::Track(t.id), t.inserts.iter().rfind(new)?.id)))
        .or_else(|| {
            track.is_none_or(|name| name == "master").then_some(())?;
            Some((
                Channel::Master,
                project.master.inserts.iter().rfind(new)?.id,
            ))
        });
    let (channel, id) = found?;
    TARGET.set(Some((
        ViewportId::from_hash_of(("insert-editor", id.0)),
        ctx.cumulative_frame_nr(),
    )));
    Some((channel, id))
}

/// In the editor `viewport`, drawn in `area`: the picture once it has settled.
pub(crate) fn tick(ctx: &egui::Context, viewport: ViewportId, area: Rect) {
    let Some(path) = path() else {
        return;
    };
    let Some((target, opened)) = TARGET.get() else {
        return;
    };
    if target != viewport || ctx.cumulative_frame_nr() != opened + SETTLE {
        return;
    }
    let scale = ctx.pixels_per_point();
    let px = move |v: f32| (v * scale).round().max(0.0) as usize;
    ctx.request_screenshot(move |image| {
        let [w, h] = image.size;
        let (x0, x1) = (px(area.left()).min(w), px(area.right()).min(w));
        let (y0, y1) = (px(area.top()).min(h), px(area.bottom()).min(h));
        let mut ppm = format!(
            "P6\n{} {}\n255\n",
            x1.saturating_sub(x0),
            y1.saturating_sub(y0)
        )
        .into_bytes();
        for y in y0..y1 {
            for x in x0..x1 {
                let p = image.pixels[y * w + x];
                ppm.extend([p.r(), p.g(), p.b()]);
            }
        }
        if let Err(e) = std::fs::write(&path, ppm) {
            tracing::error!("cannot write screenshot {}: {e}", path.display());
        }
        std::process::exit(0);
    });
}
