# Notes for coding agents

## Windowing: floating windows

AudioStrada opens windows besides the main one: detached panels (mixer, piano roll, rack),
effect editors, the effect browser, and plugin windows (built-in, third-party in process, and
third-party in a sandboxed plugin host, which is another process). They must behave the same
on every system, as Reaper's floating windows do.

### Expected behavior, on every system

- A floating window stays visible until it is closed. It never goes behind the main window.
- A detached panel never covers a window put over it, on whatever screen. For example, the
  mixer maximized on a second screen, clicked, does not hide the browser or a plugin window
  placed over it.
- Floating windows move freely from one screen to another.
- AudioStrada shows as one application in the taskbar, the Dock and Alt+Tab: floating
  windows have no entry of their own.
- Minimizing the main window hides the floating windows; restoring it brings them back.
- A window opens centered on the window it was opened from, so on that window's screen. It
  then opens where it was last left, if that is still on a screen.
- Opening a window that is already open brings it to the front. It must never get lost
  behind another window with no way to get it back.
- Floating windows never cover AudioStrada's dialogs (file choosers, messages).
- When another application is in front, floating windows do not cover it.

Test any windowing change with two screens and the mixer detached and maximized on the
second one. Open the browser and a third-party plugin from the mixer, move them over it,
click the mixer, then open them again.

When driving the real windows from a script (`SetCursorPos`, `mouse_event`), move the mouse
a few pixels before clicking: egui-winit drops a click whose window has had no cursor
position since the pointer left it, so a teleported cursor clicks into nothing and looks like
a frozen window. `GetGUIThreadInfo` (capture, active, focus windows of AudioStrada's UI
thread) and `WindowFromPoint` tell where input really goes.

### Never break the other systems

Windowing code differs on each system and each part sits behind `cfg`. A change for one
system must not alter the others. The Linux behavior (X11, and Wayland run in X11 mode) and
the macOS behavior were tuned by hand; keep them unless they are tested on those systems.

### Windows (implemented)

- **AudioStrada's own windows** (winit viewports) are owned by another window, set with
  `SetWindowLongPtrW(GWLP_HWNDPARENT)` in `own_windows`
  ([window.rs](crates/audiostrada-plugins/src/window.rs)). The UI calls it at every frame
  from `keep_windows_above` ([lib.rs](crates/audiostrada-ui/src/lib.rs)).
  - Detached panels are owned by the main window.
  - Every other window (editors, browser) is owned by the panel it overlaps most, else by the
    main window. Windows then keeps it above that panel.
- **Taskbar and Alt+Tab:** `floating_builder` sets `with_taskbar(false)` and no minimize box.
  winit also gives every window `WS_EX_APPWINDOW`, which puts even owned windows in Alt+Tab,
  and sets it again whenever a window's state changes. `own_windows` therefore removes it at
  every frame.
- **Closing a panel:** Windows destroys owned windows with their owner, behind winit's back,
  and eframe never recovers such a window. Before a panel's window closes, `release_windows`
  hands its windows to the main window (see `detached_windows`).
- **Window positions:** a position given to `ViewportBuilder` must not change from one frame
  to the next, or egui moves the window. `floating_builder` computes the centered position
  once per viewport.
- **Sandboxed plugin windows** live in the plugin host process. Never make them owned by, or
  children of, a window of AudioStrada's process: Windows would then share input between the
  two processes, and a busy plugin host would freeze AudioStrada. `HostWindow::follow` keeps
  them in place instead:
  - topmost while one of AudioStrada's winit windows, or a plugin window, is in front;
  - just behind one of AudioStrada's dialogs in front;
  - left under another application when that one is in front;
  - hidden while the main window is minimized.

  These windows use the tool window style, to stay out of the taskbar and Alt+Tab. Windows
  refuses the foreground to a background process, so AudioStrada calls `allow_foreground`
  before `OpenGui` and `RaiseGui`, and `raise` puts the window topmost by itself.
  `follow` checks the window's real topmost style rather than trusting what it last set:
  a plugin can take the style off. Some plugin processes refuse z-order changes on our frame
  window altogether once their editor was opened before (Analog Lab V: topmost, `HWND_TOP`,
  even from AudioStrada in front, all ignored; FabFilter's are fine). `follow` then retries
  twice a second at most, and clicking the plugin's slot still brings the window to the front
  (activation is not refused).
- **Plugin windows in AudioStrada's process** (sandbox off) are owned by the main window.
- **Test:** `floating_windows_are_owned_by_the_panel_under_them` in `window.rs` checks the
  ownership rules on real Win32 windows.

### Linux (X11; Wayland runs in X11 mode)

- `floating_level` sets the window levels. The browser floats; an editor floats while the
  main window, its own window or the one drawing it has the focus; panels stay at the normal
  level.
- Floating windows drop under the main window's menus, dialogs and file choosers
  (`uncover_main`).
- Plugin windows are `WM_TRANSIENT_FOR` the main window.
- Making the egui windows transient for the main window was tried and reverted (commit
  0ec8c08).

Panels can still go behind the main window here, unlike the expected behavior above. Closing
that gap needs testing under X11 and Wayland.

### macOS

AudioStrada's windows and the plugin windows float (`NSFloatingWindowLevel`) while
AudioStrada or one of its plugin hosts is the active application, and drop to the normal level
otherwise (`keep_windows_above`, `HostWindow`).
