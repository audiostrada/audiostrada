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

/// Under X11, AudioStrada's other windows (panels, editors, the effect browser) are made
/// transient for the main one and put in its group: the window manager then keeps them above
/// it, and only it, and treats them as the same application, so they take the focus when
/// they open rather than raising a "… is ready" notice. Cheap when nothing is new.
#[cfg(target_os = "linux")]
pub fn adopt_windows(main: u32) {
    use std::sync::Mutex;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    struct State {
        connection: RustConnection,
        root: u32,
        pid: u32,
        leader: u32,
        client_list: u32,
        pid_atom: u32,
        /// Windows looked at already, AudioStrada's or not.
        seen: std::collections::HashSet<u32>,
    }
    static STATE: Mutex<Option<State>> = Mutex::new(None);

    if main == 0 || !X11.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    if state.is_none() {
        let Ok((connection, screen)) = x11rb::connect(None) else {
            return;
        };
        let Some(root) = connection.setup().roots.get(screen).map(|s| s.root) else {
            return;
        };
        let atom = |name: &[u8]| {
            connection
                .intern_atom(false, name)
                .ok()
                .and_then(|c| c.reply().ok())
                .map_or(0, |r| r.atom)
        };
        let (leader, client_list) = (atom(b"WM_CLIENT_LEADER"), atom(b"_NET_CLIENT_LIST"));
        let pid_atom = atom(b"_NET_WM_PID");
        *state = Some(State {
            connection,
            root,
            pid: std::process::id(),
            leader,
            client_list,
            pid_atom,
            seen: std::collections::HashSet::new(),
        });
    }
    let Some(s) = state.as_mut() else {
        return;
    };
    let c = &s.connection;
    // Managed windows, and those not mapped yet (still children of the root).
    let mut windows: Vec<u32> = c
        .get_property(false, s.root, s.client_list, AtomEnum::WINDOW, 0, 4096)
        .ok()
        .and_then(|r| r.reply().ok())
        .and_then(|r| r.value32().map(Iterator::collect))
        .unwrap_or_default();
    if let Some(tree) = c.query_tree(s.root).ok().and_then(|r| r.reply().ok()) {
        windows.extend(tree.children);
    }
    let mut changed = false;
    if s.seen.insert(main) {
        let _ = c.change_property32(PropMode::REPLACE, main, s.leader, AtomEnum::WINDOW, &[main]);
        changed = true;
    }
    windows.retain(|w| s.seen.insert(*w));
    let ours = |w: u32| {
        c.get_property(false, w, s.pid_atom, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|r| r.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()))
            == Some(s.pid)
    };
    for w in windows {
        if !ours(w) {
            continue;
        }
        let _ = c.change_property32(
            PropMode::REPLACE,
            w,
            AtomEnum::WM_TRANSIENT_FOR,
            AtomEnum::WINDOW,
            &[main],
        );
        let _ = c.change_property32(PropMode::REPLACE, w, s.leader, AtomEnum::WINDOW, &[main]);
        changed = true;
    }
    if changed {
        let _ = c.flush();
    }
}
