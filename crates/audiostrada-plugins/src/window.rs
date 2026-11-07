//! Native top-level windows that host embedded plugin editors (X11 on Linux and BSDs, Win32 on
//! Windows, AppKit on macOS). Closing the window is reported by [`HostWindow::poll`].

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
    /// The window's content view (`NSView`).
    Cocoa(*mut std::ffi::c_void),
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

    /// Asks the window manager to bring AudioStrada's window to the front, even when it
    /// already has the focus (a plain focus request is then ignored).
    pub fn raise_parent() {
        let Ok(parent) = u32::try_from(super::parent()) else {
            return;
        };
        if parent == 0 {
            return;
        }
        let Ok((connection, screen)) = x11rb::connect(None) else {
            return;
        };
        let root = connection.setup().roots[screen].root;
        let Ok(Ok(active)) = connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .map(|c| c.reply())
        else {
            return;
        };
        // Source 2, as a pager: the window manager obeys.
        let event = ClientMessageEvent::new(32, parent, active.atom, [2, 0, 0, 0, 0]);
        let _ = connection.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        );
        let _ = connection.flush();
    }

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

    use std::sync::Mutex;

    use windows::Win32::Foundation::{
        CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO,
        MonitorFromPoint, MonitorFromWindow,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW,
        DestroyWindow, EnumThreadWindows, GWL_EXSTYLE, GWLP_HWNDPARENT, GetClassNameW,
        GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextW,
        GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindow,
        IsWindowVisible, RegisterClassW, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
        ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WNDCLASSW, WS_CAPTION,
        WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPED, WS_SYSMENU,
    };
    use windows::core::{BOOL, PCWSTR, PWSTR, w};

    use super::NativeHandle;

    const CLASS: PCWSTR = w!("AudioStradaPluginWindow");

    fn process_of(hwnd: HWND) -> u32 {
        let mut pid = 0;
        // SAFETY: plain Win32 call; an invalid window gives 0.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid
    }

    fn class_of(window: HWND) -> String {
        let mut class = [0u16; 64];
        // SAFETY: plain Win32 call; an invalid window gives an empty name.
        let length = unsafe { GetClassNameW(window, &mut class) }.max(0) as usize;
        String::from_utf16_lossy(&class[..length])
    }

    /// Lets the process `pid` (a plugin host) bring its window to the front: Windows refuses
    /// it to a process in the background unless the one in front allows it.
    pub fn allow_foreground(pid: u32) {
        // SAFETY: plain Win32 call.
        let _ = unsafe { AllowSetForegroundWindow(pid) };
    }

    fn hwnd(handle: u64) -> HWND {
        HWND(handle as usize as *mut std::ffi::c_void)
    }

    pub(super) fn in_this_process(window: u64) -> bool {
        // SAFETY: plain Win32 call.
        process_of(hwnd(window)) == unsafe { GetCurrentProcessId() }
    }

    fn same_executable(pid: u32) -> bool {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        // SAFETY: plain Win32 calls; the process handle is closed once.
        unsafe {
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut name = [0u16; 1024];
            let mut length = name.len() as u32;
            let named = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(name.as_mut_ptr()),
                &mut length,
            )
            .is_ok();
            let _ = CloseHandle(process);
            named
                && String::from_utf16_lossy(&name[..length as usize])
                    .eq_ignore_ascii_case(&exe.to_string_lossy())
        }
    }

    /// True when `window` is AudioStrada's: of this process, or of another running the same
    /// executable (AudioStrada itself or a plugin host).
    fn ours(window: HWND) -> bool {
        let pid = process_of(window);
        // SAFETY: plain Win32 call.
        if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
            return pid != 0;
        }
        static LAST: Mutex<(u32, bool)> = Mutex::new((0, false));
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.0 != pid {
            *last = (pid, same_executable(pid));
        }
        last.1
    }

    /// This thread's winit windows besides `main`, with their titles.
    fn winit_windows(main: HWND) -> Vec<(HWND, String)> {
        unsafe extern "system" fn collect(window: HWND, list: LPARAM) -> BOOL {
            // SAFETY: `list` is the vector handed to EnumThreadWindows below, alive meanwhile.
            unsafe { (*(list.0 as *mut Vec<HWND>)).push(window) };
            true.into()
        }
        let mut windows: Vec<HWND> = Vec::new();
        // SAFETY: plain Win32 calls on this thread's windows.
        unsafe {
            let _ = EnumThreadWindows(
                GetCurrentThreadId(),
                Some(collect),
                LPARAM(&mut windows as *mut Vec<HWND> as isize),
            );
            windows
                .into_iter()
                .filter(|&window| {
                    let mut class = [0u16; 32];
                    let length = GetClassNameW(window, &mut class).max(0) as usize;
                    window != main && String::from_utf16_lossy(&class[..length]) == "Window Class"
                })
                .map(|window| {
                    let mut title = [0u16; 512];
                    let length = GetWindowTextW(window, &mut title).max(0) as usize;
                    (window, String::from_utf16_lossy(&title[..length]))
                })
                .collect()
        }
    }

    fn owner_of(window: HWND) -> isize {
        // SAFETY: plain Win32 call; GWLP_HWNDPARENT of a top-level window is its owner.
        unsafe { GetWindowLongPtrW(window, GWLP_HWNDPARENT) }
    }

    fn set_owner(window: HWND, owner: HWND) {
        if owner_of(window) != owner.0 as isize {
            // SAFETY: plain Win32 call on one of this thread's windows.
            unsafe { SetWindowLongPtrW(window, GWLP_HWNDPARENT, owner.0 as isize) };
        }
    }

    fn rect_of(window: HWND) -> RECT {
        let mut rect = RECT::default();
        // SAFETY: plain Win32 call; a failure leaves an empty rectangle.
        let _ = unsafe { GetWindowRect(window, &mut rect) };
        rect
    }

    fn overlap(a: &RECT, b: &RECT) -> i64 {
        let width = i64::from(a.right.min(b.right) - a.left.max(b.left)).max(0);
        let height = i64::from(a.bottom.min(b.bottom) - a.top.max(b.top)).max(0);
        width * height
    }

    /// Makes this thread's other winit windows owned, as Reaper's floating windows are: the
    /// detached panels titled `panels` by the main window, the others (effect editors, the
    /// browser) by the panel they overlap most, else by the main window, so that a panel
    /// clicked never covers them, on whatever screen. Windows then keeps each above its owner
    /// and never behind, hides them with the main window when it is minimized, leaves them
    /// out of the taskbar and of Alt+Tab, and they still go to any screen. Cheap: call it at
    /// each frame, windows opened or moved since are owned on the next one. An owner closing
    /// takes its windows with it: see [`release_windows`].
    pub fn own_windows(main: u64, panels: &[String]) {
        let main = hwnd(main);
        let windows = winit_windows(main);
        let panels: Vec<(HWND, RECT)> = windows
            .iter()
            .filter(|(_, title)| panels.contains(title))
            .map(|&(window, _)| (window, rect_of(window)))
            .collect();
        for &(window, _) in &windows {
            let owner = if panels.iter().any(|&(p, _)| p == window) {
                main
            } else {
                let rect = rect_of(window);
                panels
                    .iter()
                    .map(|(p, r)| (*p, overlap(&rect, r)))
                    .filter(|&(_, area)| area > 0)
                    .max_by_key(|&(_, area)| area)
                    .map_or(main, |(p, _)| p)
            };
            set_owner(window, owner);
            // SAFETY: plain Win32 calls; only this thread's windows are changed.
            unsafe {
                // winit marks all its windows as application windows, which puts even owned
                // ones in Alt+Tab, and marks them again whenever their state changes (maximized,
                // resizable…): taken off at each frame.
                let extended = GetWindowLongPtrW(window, GWL_EXSTYLE);
                if extended & WS_EX_APPWINDOW.0 as isize != 0 {
                    SetWindowLongPtrW(
                        window,
                        GWL_EXSTYLE,
                        extended & !(WS_EX_APPWINDOW.0 as isize),
                    );
                }
                // What earlier versions left: topmost windows float over other applications.
                if extended & WS_EX_TOPMOST.0 as isize != 0 {
                    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                    let _ = SetWindowPos(window, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                }
            }
        }
    }

    /// Hands the windows owned by the one titled `title` to the main window, before that one
    /// closes: Windows destroys owned windows with their owner, behind winit's back, and such
    /// a window would never show again.
    pub fn release_windows(main: u64, title: &str) {
        let main = hwnd(main);
        let windows = winit_windows(main);
        let Some(&(owner, _)) = windows.iter().find(|(_, t)| t == title) else {
            return;
        };
        for &(window, _) in &windows {
            if owner_of(window) == owner.0 as isize {
                set_owner(window, main);
            }
        }
    }

    /// Where the plugin windows of this process were last left, by title: they open there
    /// again.
    static PLACES: Mutex<Vec<(String, i32, i32)>> = Mutex::new(Vec::new());

    /// Where a window of this outer size opens: where the last one so named was left if that
    /// is still on a screen, else centered on AudioStrada's window in front (the detached
    /// mixer it was opened from, say), else on its main one: on that window's screen.
    fn place(title: &str, width: i32, height: i32) -> (i32, i32) {
        let places = PLACES.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: plain Win32 calls; a closed or missing window gives the defaults.
        unsafe {
            if let Some(&(_, x, y)) = places.iter().find(|(t, ..)| t == title) {
                let corner = POINT {
                    x: x + 40,
                    y: y + 10,
                };
                if !MonitorFromPoint(corner, MONITOR_DEFAULTTONULL).is_invalid() {
                    return (x, y);
                }
            }
            let main = hwnd(super::parent());
            let front = GetForegroundWindow();
            let parent = if super::parent() != 0
                && !front.is_invalid()
                && process_of(front) == process_of(main)
                && IsWindowVisible(front).as_bool()
            {
                front
            } else {
                main
            };
            let mut main = RECT::default();
            if super::parent() == 0 || GetWindowRect(parent, &mut main).is_err() {
                return (100, 100);
            }
            let x = (main.left + main.right - width) / 2;
            let y = (main.top + main.bottom - height) / 2;
            // Kept on that screen: the title bar never out of reach.
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let monitor = MonitorFromWindow(parent, MONITOR_DEFAULTTONEAREST);
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return (x, y);
            }
            let work = info.rcWork;
            (
                x.min(work.right - width).max(work.left),
                y.min(work.bottom - height).max(work.top),
            )
        }
    }

    fn remember_place(title: &str, window: HWND) {
        let mut rect = RECT::default();
        // SAFETY: plain Win32 call on our window.
        if unsafe { GetWindowRect(window, &mut rect) }.is_err() {
            return;
        }
        let mut places = PLACES.lock().unwrap_or_else(|e| e.into_inner());
        places.retain(|(t, ..)| t != title);
        places.push((title.to_owned(), rect.left, rect.top));
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

    /// Where a followed window stands, last set by [`HostWindow::follow`].
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Stand {
        /// Topmost: AudioStrada's main window or a plugin window is in front.
        Top,
        /// Just behind one of AudioStrada's other windows (a panel, a dialog), in front.
        Behind(HWND),
        /// Left where it was when another application came in front: above AudioStrada's
        /// window, under that application.
        Away,
    }

    /// Without a minimize box: an owned window minimizes to a stub over the taskbar, and a
    /// followed one would be lost (it has no taskbar button).
    const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0);

    pub struct HostWindow {
        hwnd: HWND,
        title: String,
        /// AudioStrada's window in another process, which this one keeps above like an owned
        /// window would (see [`Self::follow`]); owning across processes would tie AudioStrada's
        /// input to a busy plugin host.
        follow: Option<HWND>,
        stand: Stand,
        /// When the window was last put topmost and did not stay so (see [`Self::follow`]).
        refused: Option<std::time::Instant>,
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
                // Out of the taskbar and of Alt+Tab: an owned window is by itself, a followed
                // one has no owner and needs the tool window style.
                let extended = if follow.is_some() {
                    WS_EX_TOOLWINDOW
                } else {
                    WINDOW_EX_STYLE::default()
                };
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                };
                let _ = AdjustWindowRectEx(&mut rect, STYLE, false, extended);
                let (outer_width, outer_height) = (rect.right - rect.left, rect.bottom - rect.top);
                let (x, y) = place(title, outer_width, outer_height);
                let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
                let hwnd = CreateWindowExW(
                    extended,
                    CLASS,
                    PCWSTR(wide.as_ptr()),
                    STYLE,
                    x,
                    y,
                    outer_width,
                    outer_height,
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
                    title: title.to_owned(),
                    follow,
                    // As created: not topmost, above AudioStrada's window.
                    stand: Stand::Away,
                    refused: None,
                    hidden: false,
                };
                window.follow();
                Ok(window)
            }
        }

        /// Keeps the window above AudioStrada's, in another process, as if owned by it, without
        /// ever waiting for it: hidden while it is minimized; topmost while AudioStrada's
        /// windows (main, panels, editors, browser) or a plugin window are in front, so that a
        /// panel clicked never covers it, on whatever screen; just behind one of AudioStrada's
        /// dialogs in front (a file chooser, a message), which it must not cover; left above
        /// AudioStrada's window, under the application in front, when another one is.
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
                // Between two windows (Alt+Tab, the desktop): nothing to follow yet.
                if front.is_invalid() {
                    return;
                }
                let main_process = process_of(audiostrada) == process_of(front);
                let wanted = if front == audiostrada
                    || (!main_process && ours(front))
                    || (main_process && class_of(front) == "Window Class")
                {
                    Stand::Top
                } else if main_process {
                    Stand::Behind(front)
                } else {
                    Stand::Away
                };
                let topmost =
                    |w: HWND| GetWindowLongPtrW(w, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0;
                // Checked on the window itself: it can lose the topmost style (a plugin's
                // doing) and would then go under the panels. Some plugins' windows refuse it
                // altogether (Analog Lab V, once its editor was opened before): tried again
                // now and then only; clicking the plugin's slot still brings it to the front.
                let lost = wanted == Stand::Top
                    && !topmost(self.hwnd)
                    && self
                        .refused
                        .is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(500));
                if wanted == self.stand && !lost {
                    return;
                }
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                match wanted {
                    Stand::Top => {
                        let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                        self.refused = (!topmost(self.hwnd)).then(std::time::Instant::now);
                    }
                    // Already under the windows in front, above AudioStrada's: left there.
                    Stand::Away if self.stand != Stand::Top => {}
                    Stand::Away | Stand::Behind(_) => {
                        // Out of the topmost band lands on top of the others: then just
                        // behind the window in front, unless it is topmost itself (a shell
                        // window), as following one would make this one topmost again.
                        let _ = SetWindowPos(self.hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                        if !topmost(front) {
                            let _ = SetWindowPos(self.hwnd, Some(front), 0, 0, 0, 0, flags);
                        }
                    }
                }
                self.stand = wanted;
            }
        }

        pub fn handle(&self) -> NativeHandle {
            NativeHandle::Win32(self.hwnd.0)
        }

        pub fn resize(&mut self, width: u32, height: u32) {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            // SAFETY: plain Win32 calls on our window.
            unsafe {
                let extended = WINDOW_EX_STYLE(GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32);
                let _ = AdjustWindowRectEx(&mut rect, STYLE, false, extended);
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

        /// Brings the window to the front: asked from AudioStrada, which is then in front. A
        /// followed window goes topmost itself, as the focus may still be refused to it.
        pub fn raise(&mut self) {
            // SAFETY: plain Win32 calls on our window.
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                if self.follow.is_some() {
                    let flags = SWP_NOMOVE | SWP_NOSIZE;
                    let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                    self.stand = Stand::Top;
                }
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
            remember_place(&self.title, self.hwnd);
            // SAFETY: our window, destroyed once.
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSFloatingWindowLevel, NSNormalWindowLevel,
        NSRunningApplication, NSView, NSWindow, NSWindowStyleMask, NSWorkspace,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    use super::NativeHandle;

    /// True while AudioStrada is the active application: the application itself or one of its
    /// plugin hosts (the same executable), whose windows take the focus from it.
    fn audiostrada_active() -> bool {
        let Some(front) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return false;
        };
        let ours = NSRunningApplication::currentApplication().executableURL();
        ours.is_some() && front.executableURL() == ours
    }

    /// Above AudioStrada's main window while AudioStrada is in use, without taking the focus
    /// from it; at the normal level behind other applications.
    fn keep_above(window: &NSWindow, active: bool) {
        let level = if active {
            NSFloatingWindowLevel
        } else {
            NSNormalWindowLevel
        };
        if window.level() != level {
            window.setLevel(level);
        }
    }

    /// Keeps the application's other windows (winit's: detached panels, editors) above its main
    /// window, given by its content view, as [`HostWindow`] does for plugin windows.
    pub fn keep_windows_above(main: *mut std::ffi::c_void) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // SAFETY: the main window's live content view, used on the main thread.
        let main = unsafe { main.cast::<NSView>().as_ref() }.and_then(|v| v.window());
        let active = audiostrada_active();
        for window in NSApplication::sharedApplication(mtm).windows() {
            if Some(&window) != main.as_ref() && window.class().name().to_bytes() == b"WinitWindow"
            {
                keep_above(&window, active);
            }
        }
    }

    /// An AppKit window whose content view holds the plugin's editor. Its process cannot be
    /// owned by AudioStrada's window (another process): it floats above AudioStrada's windows
    /// while AudioStrada is in use instead.
    pub struct HostWindow {
        window: Retained<NSWindow>,
        mtm: MainThreadMarker,
    }

    impl HostWindow {
        pub fn new(title: &str, width: u32, height: u32) -> Result<Self, String> {
            let mtm = MainThreadMarker::new().ok_or("plugin windows open on the main thread")?;
            let size = NSSize::new(f64::from(width.max(1)), f64::from(height.max(1)));
            let style = NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable;
            // SAFETY: a plain window created on the main thread.
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    NSRect::new(NSPoint::new(0.0, 0.0), size),
                    style,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            // SAFETY: we keep our own reference; closing only orders the window out.
            unsafe { window.setReleasedWhenClosed(false) };
            window.setTitle(&NSString::from_str(title));
            window.center();
            keep_above(&window, true);
            let mut host = Self { window, mtm };
            host.raise();
            Ok(host)
        }

        pub fn handle(&self) -> NativeHandle {
            let view = self.window.contentView();
            NativeHandle::Cocoa(view.map_or(std::ptr::null_mut(), |v| {
                Retained::as_ptr(&v).cast_mut().cast()
            }))
        }

        pub fn resize(&mut self, width: u32, height: u32) {
            let size = NSSize::new(f64::from(width.max(1)), f64::from(height.max(1)));
            self.window.setContentSize(size);
        }

        pub fn raise(&mut self) {
            self.window.makeKeyAndOrderFront(None);
            // `activate` only exists from macOS 14 on.
            #[allow(deprecated)]
            NSApplication::sharedApplication(self.mtm).activateIgnoringOtherApps(true);
        }

        pub fn take_keys(&mut self) -> Vec<(u32, u16)> {
            Vec::new()
        }

        /// True once the user closed the window (ordered out, not merely minimized).
        pub fn poll(&mut self) -> bool {
            keep_above(&self.window, audiostrada_active());
            !self.window.isVisible() && !self.window.isMiniaturized()
        }
    }

    impl Drop for HostWindow {
        fn drop(&mut self) {
            self.window.close();
        }
    }
}

pub use imp::HostWindow;

/// Lets a plugin host bring its window to the front (Windows; nothing elsewhere).
pub fn allow_foreground(pid: u32) {
    #[cfg(windows)]
    imp::allow_foreground(pid);
    #[cfg(not(windows))]
    let _ = pid;
}
#[cfg(target_os = "macos")]
pub use imp::keep_windows_above;
#[cfg(all(unix, not(target_os = "macos")))]
pub use imp::raise_parent;
#[cfg(windows)]
pub use imp::{own_windows, release_windows};

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

#[cfg(all(test, windows))]
#[allow(clippy::unwrap_used)]
mod windows_tests {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_EXSTYLE, GWLP_HWNDPARENT,
        GetWindowLongPtrW, IsWindow, RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE,
        SWP_NOZORDER, SetWindowPos, ShowWindow, WNDCLASSW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::{PCWSTR, w};

    use super::*;

    unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        // SAFETY: default handling, with what Windows handed us.
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    /// A window of winit's class, as AudioStrada's are, shown without the focus.
    fn window(title: &str, x: i32) -> HWND {
        static REGISTER: std::sync::Once = std::sync::Once::new();
        // SAFETY: plain Win32 calls with valid arguments, on this test's thread.
        unsafe {
            let instance = GetModuleHandleW(None).unwrap().into();
            REGISTER.call_once(|| {
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance,
                    lpszClassName: w!("Window Class"),
                    ..Default::default()
                });
            });
            let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            let hwnd = CreateWindowExW(
                WS_EX_APPWINDOW,
                w!("Window Class"),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                x,
                0,
                400,
                300,
                None,
                None,
                Some(instance),
                None,
            )
            .unwrap();
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            hwnd
        }
    }

    fn owner(window: HWND) -> isize {
        // SAFETY: plain Win32 call.
        unsafe { GetWindowLongPtrW(window, GWLP_HWNDPARENT) }
    }

    fn move_to(window: HWND, x: i32) {
        // SAFETY: plain Win32 call on a window of this test.
        unsafe {
            let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
            SetWindowPos(window, None, x, 0, 0, 0, flags).unwrap();
        }
    }

    /// The effect browser follows the panel it is put on, so that clicking that panel never
    /// covers it; it outlives the panel closing.
    #[test]
    fn floating_windows_are_owned_by_the_panel_under_them() {
        let main = window("Main — test", 0);
        let mixer = window("Mixer — test", 2000);
        let browser = window("Browser — test", 100);
        let (main_id, panels) = (main.0 as u64, vec!["Mixer — test".to_owned()]);

        imp::own_windows(main_id, &panels);
        assert_eq!(owner(mixer), main.0 as isize, "panels: by the main window");
        assert_eq!(
            owner(browser),
            main.0 as isize,
            "over the main window: by it"
        );
        // SAFETY: plain Win32 call.
        let extended = unsafe { GetWindowLongPtrW(browser, GWL_EXSTYLE) };
        assert_eq!(extended & WS_EX_APPWINDOW.0 as isize, 0, "out of Alt+Tab");

        move_to(browser, 2100);
        imp::own_windows(main_id, &panels);
        assert_eq!(
            owner(browser),
            mixer.0 as isize,
            "over the mixer: by the mixer"
        );

        move_to(browser, 100);
        imp::own_windows(main_id, &panels);
        assert_eq!(owner(browser), main.0 as isize, "back over the main window");

        move_to(browser, 2100);
        imp::own_windows(main_id, &panels);
        imp::release_windows(main_id, "Mixer — test");
        assert_eq!(
            owner(browser),
            main.0 as isize,
            "released before the mixer closes"
        );
        // SAFETY: windows of this test, destroyed once.
        unsafe {
            DestroyWindow(mixer).unwrap();
            assert!(
                IsWindow(Some(browser)).as_bool(),
                "the browser outlives the mixer"
            );
            let _ = DestroyWindow(browser);
            let _ = DestroyWindow(main);
        }
    }
}
