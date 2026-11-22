//! What the windows besides the main one must do that egui cannot tell the system by itself:
//! the menus open in each window, which the floating windows over them step aside for, as egui
//! draws a menu inside its window and no system lets it rise above another window.
//!
//! The rules are kept apart from the systems' calls ([`stepping_aside`]) so that they are
//! tested everywhere; each system applies them in its own way (see AGENTS.md, Windowing).

use std::sync::Mutex;

use eframe::egui;

/// A rectangle on the desktop in the system's units (pixels on Windows and X11, points from
/// the top left of the main screen on macOS): left, top, right, bottom.
pub(crate) type Area = [f32; 4];

/// The menus open in one window.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Menus {
    pub(crate) viewport: egui::ViewportId,
    /// The window's title, which finds it among the system's windows; `None` for the main one.
    pub(crate) title: Option<String>,
    pub(crate) areas: Vec<Area>,
}

static MENUS: Mutex<Vec<Menus>> = Mutex::new(Vec::new());

/// Watches every window's passes for open menus (once, at startup).
pub(crate) fn install(ctx: &egui::Context) {
    ctx.on_end_pass(
        "audiostrada-menus",
        std::sync::Arc::new(|ui: &mut egui::Ui| note_menus(ui.ctx())),
    );
}

/// The menus open in every window, as of their last pass.
pub(crate) fn menus() -> Vec<Menus> {
    MENUS
        .lock()
        .map_or_else(|e| e.into_inner().clone(), |m| m.clone())
}

/// Each window's pass: the menus it shows, on the desktop. A change wakes the main window,
/// whose pass applies the windows' places.
fn note_menus(ctx: &egui::Context) {
    let viewport = ctx.viewport_id();
    let areas = if ctx.any_popup_open() {
        menu_areas(ctx)
    } else {
        Vec::new()
    };
    let title = (viewport != egui::ViewportId::ROOT)
        .then(|| ctx.input(|i| i.viewport().title.clone()))
        .flatten();
    let mut all = MENUS.lock().unwrap_or_else(|e| e.into_inner());
    let before = all.iter().position(|m| m.viewport == viewport);
    let changed = match before {
        Some(k) if areas.is_empty() => {
            all.remove(k);
            true
        }
        Some(k) if all[k].areas == areas => false,
        Some(k) => {
            all[k].areas = areas;
            true
        }
        None if areas.is_empty() => false,
        None => {
            all.push(Menus {
                viewport,
                title,
                areas,
            });
            true
        }
    };
    drop(all);
    if changed {
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// The areas shown above the window's content (menus and their submenus, lists of choices),
/// on the desktop.
fn menu_areas(ctx: &egui::Context) -> Vec<Area> {
    let Some(inner) = ctx.input(|i| i.viewport().inner_rect) else {
        return Vec::new();
    };
    let scale = native_scale(ctx);
    let layers: Vec<egui::LayerId> = ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Foreground)
            .collect()
    });
    layers
        .into_iter()
        .filter_map(|l| ctx.memory(|m| m.area_rect(l.id)))
        .map(|r| {
            let r = r.translate(inner.min.to_vec2());
            [
                r.min.x * scale,
                r.min.y * scale,
                r.max.x * scale,
                r.max.y * scale,
            ]
        })
        .collect()
}

/// From egui's points to the system's units: pixels, or points with the interface's zoom on
/// macOS.
fn native_scale(ctx: &egui::Context) -> f32 {
    if cfg!(target_os = "macos") {
        ctx.zoom_factor()
    } else {
        ctx.pixels_per_point()
    }
}

/// A window's outer rectangle on the desktop, in the system's units.
pub(crate) fn outer_area(ctx: &egui::Context, viewport: egui::ViewportId) -> Option<Area> {
    let rect = ctx.input_for(viewport, |i| i.viewport().outer_rect)?;
    let scale = ctx.input_for(viewport, |i| {
        if cfg!(target_os = "macos") {
            i.viewport()
                .native_pixels_per_point
                .map_or(1.0, |n| i.pixels_per_point / n)
        } else {
            i.pixels_per_point
        }
    });
    Some([
        rect.min.x * scale,
        rect.min.y * scale,
        rect.max.x * scale,
        rect.max.y * scale,
    ])
}

pub(crate) fn overlap(a: &Area, b: &Area) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Which of the floating windows `windows` (an id and its rectangle) must step aside: those
/// over a menu of another window, which then shows it above them. A window never steps aside
/// for its own menus, nor for those of a window it covers no part of the menu of.
pub(crate) fn stepping_aside<K: Copy + PartialEq>(
    windows: &[(K, Area)],
    menus: &[(K, Vec<Area>)],
) -> Vec<(K, K)> {
    let mut aside = Vec::new();
    for &(window, rect) in windows {
        for (owner, areas) in menus {
            if *owner != window && areas.iter().any(|a| overlap(a, &rect)) {
                aside.push((window, *owner));
                break;
            }
        }
    }
    aside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_windows_over_a_menu_step_aside_for_it() {
        // The main window (0) shows a menu at its top left; an editor (1) covers it, another
        // (2) is elsewhere on the screen, a third (3) on the other screen.
        let menus = [(0, vec![[10.0, 30.0, 210.0, 400.0]])];
        let windows = [
            (1, [100.0, 100.0, 700.0, 500.0]),
            (2, [900.0, 100.0, 1500.0, 500.0]),
            (3, [2000.0, 100.0, 2600.0, 500.0]),
        ];
        assert_eq!(stepping_aside(&windows, &menus), vec![(1, 0)]);
    }

    #[test]
    fn a_window_never_steps_aside_for_its_own_menus() {
        let menus = [(1, vec![[100.0, 100.0, 300.0, 300.0]])];
        let windows = [(1, [0.0, 0.0, 800.0, 600.0])];
        assert!(stepping_aside(&windows, &menus).is_empty());
    }

    #[test]
    fn a_window_steps_aside_for_a_submenu_too() {
        let menus = [(
            0,
            vec![[0.0, 0.0, 100.0, 100.0], [100.0, 40.0, 300.0, 200.0]],
        )];
        let windows = [(1, [250.0, 150.0, 600.0, 600.0])];
        assert_eq!(stepping_aside(&windows, &menus), vec![(1, 0)]);
    }

    #[test]
    fn windows_side_by_side_do_not_overlap() {
        assert!(!overlap(
            &[0.0, 0.0, 100.0, 100.0],
            &[100.0, 0.0, 200.0, 100.0]
        ));
        assert!(overlap(
            &[0.0, 0.0, 100.0, 100.0],
            &[99.0, 99.0, 200.0, 200.0]
        ));
    }
}
