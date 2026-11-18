//! Full screen on the screen a window is on. Under X11, winit would use the screen the window
//! opened on (it does not follow moves reported by XWayland), so the screen is found here.

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;

#[cfg(target_os = "linux")]
static X11: AtomicBool = AtomicBool::new(false);

/// Whether the windows run through X11, decided once at start.
#[cfg(target_os = "linux")]
pub fn set_x11(x11: bool) {
    X11.store(x11, Ordering::Relaxed);
}

/// The command putting `window` in full screen (or back), on its own screen.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub fn fullscreen(
    ctx: &egui::Context,
    window: egui::ViewportId,
    on: bool,
) -> egui::ViewportCommand {
    #[cfg(target_os = "linux")]
    if on
        && X11.load(Ordering::Relaxed)
        && let Some((center, scale)) = ctx.input_for(window, |i| {
            let viewport = i.viewport();
            Some((
                viewport.outer_rect?.center(),
                viewport.native_pixels_per_point?,
            ))
        })
        && let Some(index) = x11_screen_at(center.x * scale, center.y * scale)
    {
        return egui::ViewportCommand::SetMonitor(index);
    }
    egui::ViewportCommand::Fullscreen(on)
}

/// Index, in winit's order (RandR CRTCs in use), of the screen holding a point in pixels.
#[cfg(target_os = "linux")]
fn x11_screen_at(x: f32, y: f32) -> Option<usize> {
    x11_screens()
        .iter()
        .position(|&[left, top, w, h]| x >= left && x < left + w && y >= top && y < top + h)
}

/// The screens in use under X11, in winit's order, as `[x, y, width, height]` in pixels.
#[cfg(target_os = "linux")]
fn x11_screens() -> Vec<[f32; 4]> {
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt;

    let Ok((connection, screen)) = x11rb::connect(None) else {
        return Vec::new();
    };
    let Some(root) = connection.setup().roots.get(screen).map(|s| s.root) else {
        return Vec::new();
    };
    let Some(resources) = connection
        .randr_get_screen_resources_current(root)
        .ok()
        .and_then(|c| c.reply().ok())
    else {
        return Vec::new();
    };
    resources
        .crtcs
        .iter()
        .filter_map(|&crtc| {
            connection
                .randr_get_crtc_info(crtc, x11rb::CURRENT_TIME)
                .ok()?
                .reply()
                .ok()
        })
        .filter(|info| info.width > 0 && info.height > 0 && !info.outputs.is_empty())
        .map(|info| {
            [
                f32::from(info.x),
                f32::from(info.y),
                f32::from(info.width),
                f32::from(info.height),
            ]
        })
        .collect()
}

/// The screens, in the units windows are placed in (pixels over the display scale); empty
/// when the system does not tell (Wayland without X11).
pub fn monitors(ctx: &egui::Context) -> Vec<egui::Rect> {
    let scale = ctx
        .input(|i| i.viewport().native_pixels_per_point)
        .unwrap_or(1.0)
        .max(0.25);
    let rect = |[x, y, w, h]: [f32; 4], per: f32| {
        egui::Rect::from_min_size(egui::pos2(x / per, y / per), egui::vec2(w / per, h / per))
    };
    #[cfg(target_os = "linux")]
    {
        if !X11.load(Ordering::Relaxed) {
            return Vec::new();
        }
        x11_screens().into_iter().map(|r| rect(r, scale)).collect()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Windows gives pixels, macOS points already.
        let per = if cfg!(windows) { scale } else { 1.0 };
        audiostrada_plugins::window::monitors()
            .into_iter()
            .map(|[x, y, w, h]| rect([x as f32, y as f32, w as f32, h as f32], per))
            .collect()
    }
}
