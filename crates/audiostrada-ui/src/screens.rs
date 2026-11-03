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
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt;

    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let resources = connection
        .randr_get_screen_resources_current(root)
        .ok()?
        .reply()
        .ok()?;
    let mut index = 0;
    for crtc in resources.crtcs {
        let info = connection
            .randr_get_crtc_info(crtc, x11rb::CURRENT_TIME)
            .ok()?
            .reply()
            .ok()?;
        if info.width == 0 || info.height == 0 || info.outputs.is_empty() {
            continue;
        }
        let (left, top) = (f32::from(info.x), f32::from(info.y));
        if x >= left
            && x < left + f32::from(info.width)
            && y >= top
            && y < top + f32::from(info.height)
        {
            return Some(index);
        }
        index += 1;
    }
    None
}
