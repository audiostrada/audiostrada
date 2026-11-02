//! Native top-level windows that host embedded plugin editors (X11 on Linux and BSDs, Win32 on
//! Windows). Closing the window is reported by [`HostWindow::poll`].

use std::sync::atomic::{AtomicU64, Ordering};

static PARENT: AtomicU64 = AtomicU64::new(0);

/// The application window that plugin windows stay above (X11 window id or HWND), 0 for none.
pub fn set_parent(parent: u64) {
    PARENT.store(parent, Ordering::Relaxed);
}

pub fn parent() -> u64 {
    PARENT.load(Ordering::Relaxed)
}

/// The parent when windows of this process may be owned by it, else 0: a Windows window
/// owned by another process's window shares its input queue, so a busy plugin host would
/// block AudioStrada. [`HostWindow`] follows such a parent instead.
pub fn owner() -> u64 {
    let parent = parent();
    #[cfg(windows)]
    if parent != 0 && !imp::in_this_process(parent) {
        return 0;
    }
    parent
}

static GRABBED: std::sync::Mutex<Vec<(u32, u16)>> = std::sync::Mutex::new(Vec::new());

/// Keys that plugin windows hand to AudioStrada rather than to the plugin (the transport's
/// shortcuts), as X keysyms (lowercase for letters) and modifier masks (Shift 1, Ctrl 4,
/// Alt 8). Only X11 windows take them.
pub fn set_grabbed_keys(keys: Vec<(u32, u16)>) {
    if let Ok(mut grabbed) = GRABBED.lock() {
        *grabbed = keys;
    }
}

pub fn grabbed_keys() -> Vec<(u32, u16)> {
    GRABBED.lock().map(|g| g.clone()).unwrap_or_default()
}

/// The native handle given to plugins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeHandle {
    X11(u32),
    Win32(*mut std::ffi::c_void),
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, CreateWindowAux,
        EventMask, GrabMode, ModMask, PropMode, StackMode, WindowClass,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    use super::NativeHandle;

    pub struct HostWindow {
        connection: RustConnection,
        window: u32,
        root: u32,
        delete: u32,
        closed: bool,
        /// Keycodes grabbed for AudioStrada and their keysyms; the presses waiting to be taken.
        grabbed: Vec<(u8, u32)>,
        pressed: Vec<(u32, u16)>,
    }

    impl HostWindow {
        pub fn new(title: &str, width: u32, height: u32) -> Result<Self, String> {
            let (connection, screen) =
                x11rb::connect(None).map_err(|e| format!("no X11 display: {e}"))?;
            let e = |e: &dyn std::fmt::Display| e.to_string();
            let root = &connection.setup().roots[screen];
            let (root_window, depth, visual, black) = (
                root.root,
                root.root_depth,
                root.root_visual,
                root.black_pixel,
            );
            let window = connection.generate_id().map_err(|x| e(&x))?;
            connection
                .create_window(
                    depth,
                    window,
                    root_window,
                    0,
                    0,
                    width.max(1) as u16,
                    height.max(1) as u16,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    visual,
                    &CreateWindowAux::new()
                        .background_pixel(black)
                        .event_mask(EventMask::STRUCTURE_NOTIFY),
                )
                .map_err(|x| e(&x))?;
            let atom = |name: &[u8]| -> Result<u32, String> {
                Ok(connection
                    .intern_atom(false, name)
                    .map_err(|x| e(&x))?
                    .reply()
                    .map_err(|x| e(&x))?
                    .atom)
            };
            let protocols = atom(b"WM_PROTOCOLS")?;
            let delete = atom(b"WM_DELETE_WINDOW")?;
            let utf8 = atom(b"UTF8_STRING")?;
            let net_name = atom(b"_NET_WM_NAME")?;
            connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    protocols,
                    AtomEnum::ATOM,
                    &[delete],
                )
                .map_err(|x| e(&x))?;
            connection
                .change_property8(
                    PropMode::REPLACE,
                    window,
                    AtomEnum::WM_NAME,
                    AtomEnum::STRING,
                    title.as_bytes(),
                )
                .map_err(|x| e(&x))?;
            connection
                .change_property8(PropMode::REPLACE, window, net_name, utf8, title.as_bytes())
                .map_err(|x| e(&x))?;
            // Kept above AudioStrada's window by the window manager, like a dialog.
            if let Ok(parent) = u32::try_from(super::parent())
                && parent != 0
            {
                connection
                    .change_property32(
                        PropMode::REPLACE,
                        window,
                        AtomEnum::WM_TRANSIENT_FOR,
                        AtomEnum::WINDOW,
                        &[parent],
                    )
                    .map_err(|x| e(&x))?;
            }
            connection.map_window(window).map_err(|x| e(&x))?;
            let grabbed = grab_keys(&connection, window);
            connection.flush().map_err(|x| e(&x))?;
            Ok(Self {
                connection,
                window,
                root: root_window,
                delete,
                closed: false,
                grabbed,
                pressed: Vec::new(),
            })
        }

        pub fn handle(&self) -> NativeHandle {
            NativeHandle::X11(self.window)
        }

        pub fn resize(&mut self, width: u32, height: u32) {
            let aux = ConfigureWindowAux::new()
                .width(width.max(1))
                .height(height.max(1));
            let _ = self.connection.configure_window(self.window, &aux);
            let _ = self.connection.flush();
        }

        /// Brings the window to the front and gives it the focus.
        pub fn raise(&mut self) {
            let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
            let _ = self.connection.configure_window(self.window, &aux);
            if let Ok(reply) = self
                .connection
                .intern_atom(false, b"_NET_ACTIVE_WINDOW")
                .map(|c| c.reply())
                && let Ok(active) = reply
            {
                // Source 1: an application asks; the window manager decides.
                let event = ClientMessageEvent::new(32, self.window, active.atom, [1, 0, 0, 0, 0]);
                let _ = self.connection.send_event(
                    false,
                    self.root,
                    EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                    event,
                );
            }
            let _ = self.connection.flush();
        }

        /// Handles pending events; true once the user closed the window.
        pub fn poll(&mut self) -> bool {
            while let Ok(Some(event)) = self.connection.poll_for_event() {
                match event {
                    Event::ClientMessage(m) if m.data.as_data32()[0] == self.delete => {
                        self.closed = true
                    }
                    Event::DestroyNotify(d) if d.window == self.window => self.closed = true,
                    Event::KeyPress(k) => {
                        if let Some(&(_, sym)) = self.grabbed.iter().find(|(c, _)| *c == k.detail) {
                            let mods = u16::from(k.state)
                                & u16::from(ModMask::SHIFT | ModMask::CONTROL | ModMask::M1);
                            self.pressed.push((sym, mods));
                        }
                    }
                    _ => {}
                }
            }
            self.closed
        }
    }

    impl HostWindow {
        /// Grabbed keys pressed since the last call, for AudioStrada's shortcuts.
        pub fn take_keys(&mut self) -> Vec<(u32, u16)> {
            std::mem::take(&mut self.pressed)
        }
    }

    /// Grabs AudioStrada's keys on `window`: pressed while it or the plugin inside has the focus,
    /// they come to us. Caps Lock and Num Lock do not matter.
    fn grab_keys(connection: &RustConnection, window: u32) -> Vec<(u8, u32)> {
        let keys = super::grabbed_keys();
        let setup = connection.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let Ok(Ok(map)) = connection
            .get_keyboard_mapping(min, max - min + 1)
            .map(|c| c.reply())
        else {
            return Vec::new();
        };
        let per = usize::from(map.keysyms_per_keycode.max(1));
        let (lock, num) = (u16::from(ModMask::LOCK), u16::from(ModMask::M2));
        let mut grabbed = Vec::new();
        for (i, syms) in map.keysyms.chunks(per).enumerate() {
            let code = min.saturating_add(i as u8);
            for &(sym, mods) in &keys {
                if !syms.iter().take(2).any(|&s| s == sym) {
                    continue;
                }
                for extra in [0, lock, num, lock | num] {
                    let _ = connection.grab_key(
                        false,
                        window,
                        ModMask::from(mods | extra),
                        code,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                    );
                }
                grabbed.push((code, sym));
            }
        }
        grabbed
    }

    impl Drop for HostWindow {
        fn drop(&mut self) {
            let _ = self.connection.destroy_window(self.window);
            let _ = self.connection.flush();
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::Once;

    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRect, CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow,
        GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindow,
        IsWindowVisible, RegisterClassW, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow,
        WINDOW_EX_STYLE, WM_CLOSE, WNDCLASSW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED,
        WS_SYSMENU,
    };
    use windows::core::{PCWSTR, w};

    use super::NativeHandle;

    const CLASS: PCWSTR = w!("AudioStradaPluginWindow");

    fn process_of(hwnd: HWND) -> u32 {
        let mut pid = 0;
        // SAFETY: plain Win32 call; an invalid window gives 0.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid
    }

    fn hwnd(handle: u64) -> HWND {
        HWND(handle as usize as *mut std::ffi::c_void)
    }

    pub(super) fn in_this_process(window: u64) -> bool {
        // SAFETY: plain Win32 call.
        process_of(hwnd(window)) == unsafe { GetCurrentProcessId() }
    }

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_CLOSE {
            // SAFETY: hwnd is our window, handed to us by Windows.
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            return LRESULT(0);
        }
        // SAFETY: default handling of every other message.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    pub struct HostWindow {
        hwnd: HWND,
        /// AudioStrada's window in another process, which this one keeps above like an owned
        /// window would: topmost while AudioStrada is in front, hidden while it is minimized.
        follow: Option<HWND>,
        topmost: bool,
        hidden: bool,
    }

    impl HostWindow {
        pub fn new(title: &str, width: u32, height: u32) -> Result<Self, String> {
            let parent = super::parent();
            let follow = (parent != 0 && super::owner() == 0).then(|| hwnd(parent));
            static REGISTER: Once = Once::new();
            // SAFETY: plain Win32 calls with valid arguments.
            unsafe {
                let instance: HINSTANCE = GetModuleHandleW(None).map_err(|e| e.to_string())?.into();
                REGISTER.call_once(|| {
                    let class = WNDCLASSW {
                        lpfnWndProc: Some(procedure),
                        hInstance: instance,
                        lpszClassName: CLASS,
                        ..Default::default()
                    };
                    RegisterClassW(&class);
                });
                let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                };
                let _ = AdjustWindowRect(&mut rect, style, false);
                let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    CLASS,
                    PCWSTR(title.as_ptr()),
                    style,
                    100,
                    100,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    // Owned by AudioStrada's window, so it stays above it.
                    (super::owner() != 0).then(|| hwnd(super::owner())),
                    None,
                    Some(instance),
                    None,
                )
                .map_err(|e| e.to_string())?;
                let _ = ShowWindow(hwnd, SW_SHOW);
                let mut window = Self {
                    hwnd,
                    follow,
                    topmost: false,
                    hidden: false,
                };
                window.follow();
                Ok(window)
            }
        }

        /// Keeps the window above AudioStrada's, in another process, without ever waiting for it.
        fn follow(&mut self) {
            let Some(audiostrada) = self.follow else {
                return;
            };
            // SAFETY: plain Win32 calls; only our own window is changed.
            unsafe {
                if !IsWindow(Some(audiostrada)).as_bool() {
                    return;
                }
                let minimized = IsIconic(audiostrada).as_bool();
                if minimized != self.hidden {
                    let _ = ShowWindow(
                        self.hwnd,
                        if minimized {
                            SW_HIDE
                        } else {
                            SW_SHOWNOACTIVATE
                        },
                    );
                    self.hidden = minimized;
                }
                let front = GetForegroundWindow();
                let ours =
                    [process_of(audiostrada), GetCurrentProcessId()].contains(&process_of(front));
                if ours == self.topmost {
                    return;
                }
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                if ours {
                    let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                } else {
                    // Behind the application now in front, not above it.
                    let _ = SetWindowPos(self.hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                    let _ = SetWindowPos(self.hwnd, Some(front), 0, 0, 0, 0, flags);
                }
                self.topmost = ours;
            }
        }

        pub fn handle(&self) -> NativeHandle {
            NativeHandle::Win32(self.hwnd.0)
        }

        pub fn resize(&mut self, width: u32, height: u32) {
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            // SAFETY: plain Win32 calls on our window.
            unsafe {
                let _ = AdjustWindowRect(&mut rect, style, false);
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOMOVE | SWP_NOZORDER,
                );
            }
        }

        pub fn raise(&mut self) {
            // SAFETY: plain Win32 calls on our window.
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }

        pub fn take_keys(&mut self) -> Vec<(u32, u16)> {
            Vec::new()
        }

        /// True once the user closed the window (it is hidden on close).
        pub fn poll(&mut self) -> bool {
            self.follow();
            // SAFETY: plain Win32 call on our window.
            !self.hidden && unsafe { !IsWindowVisible(self.hwnd).as_bool() }
        }
    }

    impl Drop for HostWindow {
        fn drop(&mut self) {
            // SAFETY: our window, destroyed once.
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::NativeHandle;

    pub struct HostWindow;

    impl HostWindow {
        pub fn new(_title: &str, _width: u32, _height: u32) -> Result<Self, String> {
            Err("embedded plugin windows are not supported on macOS yet".into())
        }

        pub fn handle(&self) -> NativeHandle {
            NativeHandle::Win32(std::ptr::null_mut())
        }

        pub fn resize(&mut self, _width: u32, _height: u32) {}

        pub fn raise(&mut self) {}

        pub fn take_keys(&mut self) -> Vec<(u32, u16)> {
            Vec::new()
        }

        pub fn poll(&mut self) -> bool {
            true
        }
    }
}

pub use imp::HostWindow;

impl std::fmt::Debug for HostWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostWindow")
            .field("handle", &self.handle())
            .finish()
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        ConnectionExt, CreateWindowAux, EventMask, InputFocus, WindowClass,
    };
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::wrapper::ConnectionExt as _;

    use super::*;

    /// Space pressed while a plugin's window inside ours has the focus comes to AudioStrada, not to
    /// the plugin. Needs AUDIOSTRADA_TEST_X11_KEYS and a display to play with (Xvfb).
    #[test]
    fn plugin_windows_hand_back_audiostradas_keys() {
        if std::env::var_os("AUDIOSTRADA_TEST_X11_KEYS").is_none() {
            eprintln!("AUDIOSTRADA_TEST_X11_KEYS not set, skipping");
            return;
        }
        set_grabbed_keys(vec![(0x20, 0)]);
        let mut host = HostWindow::new("test", 200, 100).unwrap();
        let NativeHandle::X11(parent) = host.handle() else {
            unreachable!()
        };
        // The plugin: a child window listening to keys, with the focus.
        let (plugin, screen) = x11rb::connect(None).unwrap();
        let child = plugin.generate_id().unwrap();
        let root = &plugin.setup().roots[screen];
        plugin
            .create_window(
                root.root_depth,
                child,
                parent,
                0,
                0,
                200,
                100,
                0,
                WindowClass::INPUT_OUTPUT,
                root.root_visual,
                &CreateWindowAux::new().event_mask(EventMask::KEY_PRESS),
            )
            .unwrap();
        plugin.map_window(child).unwrap();
        plugin.sync().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        plugin
            .set_input_focus(InputFocus::PARENT, child, x11rb::CURRENT_TIME)
            .unwrap();
        let map = plugin
            .get_keyboard_mapping(plugin.setup().min_keycode, 200)
            .unwrap()
            .reply()
            .unwrap();
        let per = usize::from(map.keysyms_per_keycode);
        let space = plugin.setup().min_keycode
            + map
                .keysyms
                .chunks(per)
                .position(|s| s.first() == Some(&0x20))
                .unwrap() as u8;
        // KeyPress is 2, KeyRelease 3.
        for kind in [2, 3] {
            plugin
                .xtest_fake_input(kind, space, x11rb::CURRENT_TIME, root.root, 0, 0, 0)
                .unwrap();
        }
        plugin.sync().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        host.poll();
        assert_eq!(host.take_keys(), vec![(0x20, 0)], "AudioStrada gets Space");
        let mut plugin_got_it = false;
        while let Ok(Some(event)) = plugin.poll_for_event() {
            plugin_got_it |= matches!(event, Event::KeyPress(_));
        }
        assert!(!plugin_got_it, "the plugin does not");
    }
}
