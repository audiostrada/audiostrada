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
    // On macOS the parent is a window number, which nothing can be owned by.
    if cfg!(target_os = "macos") {
        return 0;
    }
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
/// Alt 8). X11 and Windows plugin windows take them.
pub fn set_grabbed_keys(keys: Vec<(u32, u16)>) {
    if let Ok(mut grabbed) = GRABBED.lock() {
        *grabbed = keys;
    }
}

pub fn grabbed_keys() -> Vec<(u32, u16)> {
    GRABBED.lock().map_or_default(|g| g.clone())
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
        activate(&connection, root, parent);
    }

    /// Brings the window of this process titled `title` to the front, as [`raise_parent`].
    pub fn raise_titled(title: &str) {
        let Ok((connection, screen)) = x11rb::connect(None) else {
            return;
        };
        let root = connection.setup().roots[screen].root;
        if let Some(window) = own_clients(&connection, root)
            .into_iter()
            .find(|&w| name_of(&connection, w).as_deref() == Some(title))
        {
            activate(&connection, root, window);
        }
    }

    fn atom(connection: &RustConnection, name: &[u8]) -> Option<u32> {
        Some(connection.intern_atom(false, name).ok()?.reply().ok()?.atom)
    }

    /// The server's time, read from a property change on a window of our own: Mutter refuses
    /// an activation stamped 0 coming after a newer user action.
    fn server_time(connection: &RustConnection, root: u32) -> u32 {
        let Ok(window) = connection.generate_id() else {
            return 0;
        };
        let aux = CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE);
        let created = connection.create_window(
            0,
            window,
            root,
            -1,
            -1,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &aux,
        );
        let Some(name) = atom(connection, b"_AUDIOSTRADA_TIME") else {
            return 0;
        };
        if created.is_err()
            || connection
                .change_property8(PropMode::APPEND, window, name, AtomEnum::STRING, &[])
                .is_err()
            || connection.flush().is_err()
        {
            return 0;
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        let mut time = 0;
        while std::time::Instant::now() < deadline {
            match connection.poll_for_event() {
                Ok(Some(Event::PropertyNotify(e))) if e.window == window => {
                    time = e.time;
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
                Err(_) => break,
            }
        }
        let _ = connection.destroy_window(window);
        time
    }

    fn activate(connection: &RustConnection, root: u32, window: u32) {
        let Some(active) = atom(connection, b"_NET_ACTIVE_WINDOW") else {
            return;
        };
        let time = server_time(connection, root);
        // Source 2, as a pager: the window manager obeys.
        let event = ClientMessageEvent::new(32, window, active, [2, time, 0, 0, 0]);
        // Checked: XWayland may drop a request of a connection closed right after it.
        let _ = connection
            .send_event(
                false,
                root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map(|c| c.check());
    }

    fn cardinals(connection: &RustConnection, window: u32, name: &[u8]) -> Vec<u32> {
        let Some(property) = atom(connection, name) else {
            return Vec::new();
        };
        connection
            .get_property(false, window, property, AtomEnum::ANY, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn name_of(connection: &RustConnection, window: u32) -> Option<String> {
        let property = atom(connection, b"_NET_WM_NAME")?;
        let reply = connection
            .get_property(false, window, property, AtomEnum::ANY, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        String::from_utf8(reply.value).ok()
    }

    /// The managed windows of this process, as the window manager lists them.
    fn own_clients(connection: &RustConnection, root: u32) -> Vec<u32> {
        let pid = std::process::id();
        cardinals(connection, root, b"_NET_CLIENT_LIST")
            .into_iter()
            .filter(|&w| cardinals(connection, w, b"_NET_WM_PID").first() == Some(&pid))
            .collect()
    }

    /// The states keeping a window out of the taskbar, the pager and the window switcher
    /// (Alt+Tab): AudioStrada shows there as its main window only.
    const SKIPPED: [&[u8]; 3] = [
        b"_NET_WM_STATE_SKIP_TASKBAR",
        b"_NET_WM_STATE_SKIP_PAGER",
        b"_KDE_NET_WM_STATE_SKIP_SWITCHER",
    ];

    /// What [`follow_main`] found.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Followed {
        /// AudioStrada, or one of its plugin windows, is the active window.
        pub active: bool,
        /// Titles of the panels minimized while the main window is not.
        pub minimized: Vec<String>,
    }

    struct Follower {
        connection: RustConnection,
        root: u32,
        /// Windows looked at: whether they are this process's, and whether we made them
        /// transient for the main window.
        ours: std::collections::HashMap<u32, bool>,
        transient: std::collections::HashMap<u32, bool>,
        skipped: std::collections::HashSet<u32>,
        /// A window just back above the main window, after which the active one is raised.
        reraise: bool,
        /// A window back from aside for another window's menu (see [`window_back_from_aside`]).
        back: bool,
        /// Titles of the windows maximized: the window manager forgets it while they are
        /// unmapped with the main window minimized.
        maximized: std::collections::HashSet<String>,
        /// When the main window was last seen minimized: windows mapped again with it are
        /// not brought to the front, as a window just opened is.
        hidden_at: Option<std::time::Instant>,
        /// The last answer, given again for 100 ms while `behind` stays the same.
        last: Option<(std::time::Instant, Vec<String>, Followed)>,
        /// The menu last published (see [`publish_menu`]).
        published: Option<(Option<String>, [i32; 4])>,
    }

    thread_local! {
        static FOLLOWER: std::cell::RefCell<Option<Follower>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Keeps this process's windows besides `main` above it, as Windows does with owned
    /// windows: transient for it (Mutter then refuses no focus between them), out of the
    /// taskbar, the pager and the window switcher. Those titled in `behind` are not, so that
    /// they can step behind it for one of its menus. A few requests per window.
    pub fn follow_main(main: u64, behind: &[String], panels: &[String]) -> Followed {
        let Ok(main) = u32::try_from(main) else {
            return Followed::default();
        };
        if main == 0 {
            return Followed::default();
        }
        with_follower(|f| f.follow(main, behind, panels)).unwrap_or_default()
    }

    /// A window got its level back after stepping aside for a menu: it may cover the window
    /// opened from that menu, which [`follow_main`] brings to the front again on its next call.
    pub fn window_back_from_aside() {
        with_follower(|f| f.back = true);
    }

    /// The property of the main window naming the window showing a menu and the menu's area
    /// on the desktop (left, top, right, bottom): plugin windows over it step aside.
    const MENU: &[u8] = b"_AUDIOSTRADA_MENU";

    /// Tells the plugin hosts the menu open in one of this process's windows (`None` for the
    /// main one), as the union of its areas; `None` once it closes. Writes on change only.
    pub fn publish_menu(main: u64, menu: Option<(Option<&str>, [i32; 4])>) {
        let Ok(main) = u32::try_from(main) else {
            return;
        };
        if main == 0 {
            return;
        }
        with_follower(|f| f.publish_menu(main, menu));
    }

    fn with_follower<T>(run: impl FnOnce(&mut Follower) -> T) -> Option<T> {
        FOLLOWER.with(|f| {
            let mut f = f.borrow_mut();
            if f.is_none() {
                let (connection, screen) = x11rb::connect(None).ok()?;
                let root = connection.setup().roots[screen].root;
                *f = Some(Follower {
                    connection,
                    root,
                    ours: Default::default(),
                    transient: Default::default(),
                    skipped: Default::default(),
                    reraise: false,
                    back: false,
                    maximized: Default::default(),
                    hidden_at: None,
                    last: None,
                    published: None,
                });
            }
            f.as_mut().map(run)
        })
    }

    /// The menu published by AudioStrada's main window `parent`: the window showing it, and
    /// its area.
    fn menu_of(connection: &RustConnection, parent: u32) -> Option<(u32, [i32; 4])> {
        let v = cardinals(connection, parent, MENU);
        (v.len() == 5).then(|| (v[0], [v[1], v[2], v[3], v[4]].map(|x| x as i32)))
    }

    impl Follower {
        fn publish_menu(&mut self, main: u32, menu: Option<(Option<&str>, [i32; 4])>) {
            let menu = menu.map(|(t, a)| (t.map(str::to_owned), a));
            if menu == self.published {
                return;
            }
            let c = &self.connection;
            let Some(property) = atom(c, MENU) else {
                return;
            };
            let window = match &menu {
                Some((Some(title), _)) => own_clients(c, self.root)
                    .into_iter()
                    .find(|&w| name_of(c, w).as_deref() == Some(title.as_str())),
                Some((None, _)) => Some(main),
                None => None,
            };
            let _ = match (window, &menu) {
                (Some(w), Some((_, a))) => c
                    .change_property32(
                        PropMode::REPLACE,
                        main,
                        property,
                        AtomEnum::CARDINAL,
                        &[w, a[0] as u32, a[1] as u32, a[2] as u32, a[3] as u32],
                    )
                    .map(drop),
                _ => c.delete_property(main, property).map(drop),
            };
            let _ = c.flush();
            self.published = menu;
        }

        fn follow(&mut self, main: u32, behind: &[String], panels: &[String]) -> Followed {
            if let Some((at, b, followed)) = &self.last
                && b == behind
                && at.elapsed() < std::time::Duration::from_millis(100)
            {
                return followed.clone();
            }
            let followed = self.look(main, behind, panels);
            self.last = Some((std::time::Instant::now(), behind.to_vec(), followed.clone()));
            followed
        }

        fn look(&mut self, main: u32, behind: &[String], panels: &[String]) -> Followed {
            let c = &self.connection;
            let pid = std::process::id();
            let clients = cardinals(c, self.root, b"_NET_CLIENT_LIST");
            self.ours.retain(|w, _| clients.contains(w));
            self.transient.retain(|w, _| clients.contains(w));
            self.skipped.retain(|w| clients.contains(w));
            let mut windows = Vec::new();
            for &w in &clients {
                let ours = *self
                    .ours
                    .entry(w)
                    .or_insert_with(|| cardinals(c, w, b"_NET_WM_PID").first() == Some(&pid));
                if ours && w != main {
                    windows.push(w);
                }
            }
            let (Some(state), Some(hidden)) =
                (atom(c, b"_NET_WM_STATE"), atom(c, b"_NET_WM_STATE_HIDDEN"))
            else {
                return Followed::default();
            };
            let main_hidden = cardinals(c, main, b"_NET_WM_STATE").contains(&hidden);
            if main_hidden {
                self.hidden_at = Some(std::time::Instant::now());
            }
            let restoring = self
                .hidden_at
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(2));
            let mut opened = None;
            let skipped: Vec<u32> = SKIPPED.iter().filter_map(|n| atom(c, n)).collect();
            let maximize: Vec<u32> = [
                b"_NET_WM_STATE_MAXIMIZED_VERT",
                b"_NET_WM_STATE_MAXIMIZED_HORZ",
            ]
            .iter()
            .filter_map(|n| atom(c, *n))
            .collect();
            let mut minimized = Vec::new();
            let mut lifted = false;
            for w in windows {
                let name = name_of(c, w).unwrap_or_default();
                let want = !behind.contains(&name);
                if self.transient.get(&w) != Some(&want) {
                    let _ = if want {
                        c.change_property32(
                            PropMode::REPLACE,
                            w,
                            AtomEnum::WM_TRANSIENT_FOR,
                            AtomEnum::WINDOW,
                            &[main],
                        )
                        .map(drop)
                    } else {
                        c.delete_property(w, AtomEnum::WM_TRANSIENT_FOR.into())
                            .map(drop)
                    };
                    lifted |= want && self.transient.contains_key(&w);
                    self.transient.insert(w, want);
                }
                let states = cardinals(c, w, b"_NET_WM_STATE");
                let full = maximize.iter().all(|m| states.contains(m));
                if self.skipped.insert(w) {
                    opened = Some(w);
                    let mut added: Vec<u32> = skipped
                        .iter()
                        .copied()
                        .filter(|s| !states.contains(s))
                        .collect();
                    if !full && self.maximized.contains(&name) {
                        added.extend(maximize.iter().copied());
                    }
                    for &skip in &added {
                        // Action 1: add.
                        let event = ClientMessageEvent::new(32, w, state, [1, skip, 0, 1, 0]);
                        let _ = c.send_event(
                            false,
                            self.root,
                            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                            event,
                        );
                    }
                } else if !main_hidden && full {
                    self.maximized.insert(name.clone());
                } else if !main_hidden {
                    self.maximized.remove(&name);
                }
                if !main_hidden && states.contains(&hidden) && panels.contains(&name) {
                    minimized.push(name);
                }
            }
            let active_window = cardinals(c, self.root, b"_NET_ACTIVE_WINDOW")
                .first()
                .copied()
                .unwrap_or(0);
            let active = active_window != 0
                && (self.ours.get(&active_window) == Some(&true)
                    || cardinals(c, active_window, b"WM_TRANSIENT_FOR").first() == Some(&main));
            // A window back above the main one may cover the window opened since (About from
            // the menu it stepped aside for): that one comes up again on the next call.
            if self.reraise && active && active_window != main {
                activate(c, self.root, active_window);
            }
            // The last window opened comes to the front with the focus, whatever its screen:
            // its own request may come before it is transient, and then be refused.
            if let Some(w) = opened
                && self.last.is_some()
                && active
                && !restoring
                && !main_hidden
                && !behind.contains(&name_of(c, w).unwrap_or_default())
            {
                activate(c, self.root, w);
            }
            self.reraise = lifted || std::mem::take(&mut self.back);
            let _ = c.flush();
            Followed { active, minimized }
        }
    }

    /// True while AudioStrada's main window `parent` is minimized.
    fn minimized(connection: &RustConnection, parent: u32) -> bool {
        let Some(hidden) = atom(connection, b"_NET_WM_STATE_HIDDEN") else {
            return false;
        };
        cardinals(connection, parent, b"_NET_WM_STATE").contains(&hidden)
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
        /// Unmapped while AudioStrada's main window is minimized; when it was last checked.
        hidden: bool,
        checked: std::time::Instant,
        /// Floating above other windows, as AudioStrada's own are while it is in front.
        above: bool,
        /// Stepped aside for a menu of this window of AudioStrada's (see [`publish_menu`]).
        aside: Option<u32>,
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
            // Out of the taskbar, the pager and Alt+Tab, set before it is mapped.
            let skipped: Vec<u32> = SKIPPED.iter().filter_map(|n| atom(n).ok()).collect();
            if let Ok(state) = atom(b"_NET_WM_STATE") {
                connection
                    .change_property32(PropMode::REPLACE, window, state, AtomEnum::ATOM, &skipped)
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
                hidden: false,
                checked: std::time::Instant::now(),
                above: false,
                aside: None,
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
            let _ = self.connection.flush();
            // Another connection: reading the server's time there takes no event of ours.
            if let Ok((connection, _)) = x11rb::connect(None) {
                activate(&connection, self.root, self.window);
            }
        }

        /// Floats while AudioStrada's main window, one of its windows or a plugin window
        /// is active, as AudioStrada's floating windows do; under another application else.
        fn follow_parent(&mut self, parent: u32) {
            let c = &self.connection;
            let active = cardinals(c, self.root, b"_NET_ACTIVE_WINDOW")
                .first()
                .copied()
                .unwrap_or(0);
            let pid = |w| cardinals(c, w, b"_NET_WM_PID").first().copied();
            let ours = active != 0
                && (active == parent
                    || pid(active).is_some_and(|p| Some(p) == pid(parent))
                    || cardinals(c, active, b"WM_TRANSIENT_FOR").first() == Some(&parent));
            if ours == self.above {
                return;
            }
            let (Some(state), Some(above)) =
                (atom(c, b"_NET_WM_STATE"), atom(c, b"_NET_WM_STATE_ABOVE"))
            else {
                return;
            };
            self.above = ours;
            let event =
                ClientMessageEvent::new(32, self.window, state, [u32::from(ours), above, 0, 1, 0]);
            let _ = c.send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            );
        }

        /// Steps behind AudioStrada's window showing a menu this one covers part of, and
        /// comes back once it closes, behind the window active then (one opened from the menu).
        fn step_aside(&mut self, parent: u32) {
            let c = &self.connection;
            let frame = (|| {
                let at = c
                    .translate_coordinates(self.window, self.root, 0, 0)
                    .ok()?
                    .reply()
                    .ok()?;
                let size = c.get_geometry(self.window).ok()?.reply().ok()?;
                let (x, y) = (i32::from(at.dst_x), i32::from(at.dst_y));
                Some([x, y, x + i32::from(size.width), y + i32::from(size.height)])
            })();
            let over = menu_of(c, parent).filter(|&(w, a)| {
                w != self.window
                    && frame
                        .is_some_and(|f| f[0] < a[2] && a[0] < f[2] && f[1] < a[3] && a[1] < f[3])
            });
            let Some(state) = atom(c, b"_NET_WM_STATE") else {
                return;
            };
            let send = |data: [u32; 5]| {
                let event = ClientMessageEvent::new(32, self.window, state, data);
                let _ = c.send_event(
                    false,
                    self.root,
                    EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                    event,
                );
            };
            match (over, self.aside) {
                (Some((menu, _)), None) => {
                    if let Some(above) = atom(c, b"_NET_WM_STATE_ABOVE") {
                        send([0, above, 0, 1, 0]);
                    }
                    self.above = false;
                    // Transient for the main window, it could not go behind it.
                    if menu == parent {
                        let _ = c.delete_property(self.window, AtomEnum::WM_TRANSIENT_FOR.into());
                    }
                    let _ = c.flush();
                    if let Ok((other, _)) = x11rb::connect(None) {
                        activate(&other, self.root, menu);
                    }
                    self.aside = Some(menu);
                }
                (None, Some(_)) => {
                    let _ = c.change_property32(
                        PropMode::REPLACE,
                        self.window,
                        AtomEnum::WM_TRANSIENT_FOR,
                        AtomEnum::WINDOW,
                        &[parent],
                    );
                    self.aside = None;
                    self.follow_parent(parent);
                    let c = &self.connection;
                    let _ = c.sync();
                    let active = cardinals(c, self.root, b"_NET_ACTIVE_WINDOW")
                        .first()
                        .copied()
                        .unwrap_or(0);
                    if self.above
                        && active != 0
                        && active != self.window
                        && let Ok((other, _)) = x11rb::connect(None)
                    {
                        activate(&other, self.root, active);
                    }
                }
                _ => {}
            }
        }

        /// Handles pending events; true once the user closed the window. Hides it while
        /// AudioStrada's main window is minimized, and shows it again after.
        pub fn poll(&mut self) -> bool {
            if self.checked.elapsed() >= std::time::Duration::from_millis(200) {
                self.checked = std::time::Instant::now();
                let parent = u32::try_from(super::parent()).unwrap_or(0);
                let minimized = parent != 0 && minimized(&self.connection, parent);
                if minimized != self.hidden {
                    self.hidden = minimized;
                    if minimized {
                        let _ = self.connection.unmap_window(self.window);
                    } else {
                        // A withdrawn window may lose its states: set again before mapping.
                        let c = &self.connection;
                        let skipped: Vec<u32> = SKIPPED.iter().filter_map(|n| atom(c, n)).collect();
                        if let Some(state) = atom(c, b"_NET_WM_STATE") {
                            let _ = c.change_property32(
                                PropMode::REPLACE,
                                self.window,
                                state,
                                AtomEnum::ATOM,
                                &skipped,
                            );
                        }
                        self.above = false;
                        let _ = c.map_window(self.window);
                    }
                }
                if parent != 0 && !minimized {
                    self.step_aside(parent);
                    if self.aside.is_none() {
                        self.follow_parent(parent);
                    }
                }
                let _ = self.connection.flush();
            }
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
    use std::cell::{Cell, RefCell};
    use std::sync::Once;

    use std::sync::Mutex;

    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
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
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW,
        DestroyWindow, EnumThreadWindows, EnumWindows, GWL_EXSTYLE, GWLP_HWNDPARENT, GetClassNameW,
        GetForegroundWindow, GetPropW, GetWindowLongPtrW, GetWindowRect, GetWindowTextW,
        GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, IsIconic, IsWindow,
        IsWindowVisible, IsZoomed, RegisterClassW, RemovePropW, SW_HIDE, SW_MAXIMIZE, SW_SHOW,
        SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetForegroundWindow, SetPropW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WNDCLASSW, WS_CAPTION, WS_EX_APPWINDOW,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPED, WS_SYSMENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GA_ROOT, GetAncestor, MSG, SetWindowsHookExW, WH_GETMESSAGE, WM_KEYDOWN,
        WM_NULL, WM_SYSKEYDOWN,
    };
    use windows::core::{BOOL, PCWSTR, PWSTR, w};

    use super::NativeHandle;

    const CLASS: PCWSTR = w!("AudioStradaPluginWindow");

    thread_local! {
        /// The plugin windows (our frames) of this thread, and AudioStrada's keys pressed in
        /// them, by frame: keysym and modifiers.
        static FRAMES: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
        static PRESSED: RefCell<Vec<(isize, u32, u16)>> = const { RefCell::new(Vec::new()) };
        static HOOKED: Cell<bool> = const { Cell::new(false) };
    }

    /// Takes AudioStrada's keys (the transport's shortcuts) from the messages of this thread's
    /// plugin windows, whichever of their windows has the focus: a plugin never sees them, as
    /// in Reaper, and Space plays whatever plugin window is in front. The hook watches this
    /// thread's own messages only: in AudioStrada's process, its windows are left alone.
    fn watch_keys(frame: HWND) {
        FRAMES.with(|f| f.borrow_mut().push(frame.0 as isize));
        HOOKED.with(|hooked| {
            if !hooked.get() {
                // SAFETY: a hook on this thread's own messages, kept for the thread's life.
                let set = unsafe {
                    SetWindowsHookExW(WH_GETMESSAGE, Some(get_message), None, GetCurrentThreadId())
                };
                hooked.set(set.is_ok());
            }
        });
    }

    fn forget_keys(frame: HWND) {
        let frame = frame.0 as isize;
        FRAMES.with(|f| f.borrow_mut().retain(|&w| w != frame));
        PRESSED.with(|p| p.borrow_mut().retain(|&(w, ..)| w != frame));
    }

    unsafe extern "system" fn get_message(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // Messages leaving the queue only (not those peeked at): each one once.
        if code >= 0 && wparam.0 == 1 && lparam.0 != 0 {
            // SAFETY: for this hook, `lparam` points to the message, which it may change.
            let msg = unsafe { &mut *(lparam.0 as *mut MSG) };
            if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
                take_key(msg);
            }
        }
        // SAFETY: handing on to the next hook, as every hook does.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Keeps a key pressed in one of our plugin windows when it is one of AudioStrada's; the
    /// plugin is handed nothing instead (nor the character it would type).
    fn take_key(msg: &mut MSG) {
        // SAFETY: plain Win32 call; an invalid window gives none.
        let frame = unsafe { GetAncestor(msg.hwnd, GA_ROOT) }.0 as isize;
        if !FRAMES.with(|f| f.borrow().contains(&frame)) {
            return;
        }
        // SAFETY: plain Win32 call, the state of the keyboard as this message saw it.
        let down = |key: i32| unsafe { GetKeyState(key) } < 0;
        let mods =
            u16::from(down(0x10)) | (u16::from(down(0x11)) << 2) | (u16::from(down(0x12)) << 3);
        let key = msg.wParam.0 as u32;
        let Some((sym, _)) = super::grabbed_keys()
            .into_iter()
            .find(|&(sym, m)| m == mods && virtual_key(sym) == Some(key))
        else {
            return;
        };
        // A key held down repeats: one press only.
        if msg.lParam.0 & (1 << 30) == 0 {
            PRESSED.with(|p| p.borrow_mut().push((frame, sym, mods)));
        }
        msg.message = WM_NULL;
    }

    /// The Windows virtual key of an X keysym, among those AudioStrada's shortcuts use.
    fn virtual_key(sym: u32) -> Option<u32> {
        Some(match sym {
            // Letters (lowercase keysyms), digits, Space.
            0x61..=0x7a => sym - 0x20,
            0x30..=0x39 | 0x20 => sym,
            0xff0d => 0x0D,
            0xff1b => 0x1B,
            0xff09 => 0x09,
            0xff08 => 0x08,
            0xffff => 0x2E,
            0xff63 => 0x2D,
            0xff50 => 0x24,
            0xff57 => 0x23,
            0xff55 => 0x21,
            0xff56 => 0x22,
            0xff51 => 0x25,
            0xff52 => 0x26,
            0xff53 => 0x27,
            0xff54 => 0x28,
            0x2c => 0xBC,
            0x2e => 0xBE,
            0x2d => 0xBD,
            0x2b | 0x3d => 0xBB,
            0x2f => 0xBF,
            0x3b => 0xBA,
            0x5b => 0xDB,
            0x5d => 0xDD,
            0xffbe..=0xffc9 => 0x70 + (sym - 0xffbe),
            _ => return None,
        })
    }

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

    /// This thread's plugin windows (sandbox off), which float as AudioStrada's own windows do.
    fn plugin_frames() -> Vec<HWND> {
        FRAMES.with(|f| {
            f.borrow()
                .iter()
                .map(|&w| HWND(w as *mut std::ffi::c_void))
                .collect()
        })
    }

    /// The desktop's top-level windows, from the front to the back.
    fn z_order() -> Vec<HWND> {
        unsafe extern "system" fn collect(window: HWND, list: LPARAM) -> BOOL {
            // SAFETY: `list` is the vector handed to EnumWindows below, alive meanwhile.
            unsafe { (*(list.0 as *mut Vec<HWND>)).push(window) };
            true.into()
        }
        let mut windows: Vec<HWND> = Vec::new();
        // SAFETY: the callback only writes into `windows`, which outlives the call.
        let _ = unsafe {
            EnumWindows(
                Some(collect),
                LPARAM(&mut windows as *mut Vec<HWND> as isize),
            )
        };
        windows
    }

    fn is_topmost(window: HWND) -> bool {
        // SAFETY: plain Win32 call; an invalid window gives no style.
        unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0 }
    }

    /// Puts `window` just behind `front`, or on top of the topmost band for `None`, without
    /// activating it.
    fn place_behind(window: HWND, front: Option<HWND>) {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        // SAFETY: plain Win32 calls on one of this thread's windows.
        unsafe {
            match front {
                None => {
                    let _ = SetWindowPos(window, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                }
                Some(front) => {
                    // Out of the topmost band first, unless the window in front is topmost.
                    if is_topmost(window) && !is_topmost(front) {
                        let _ = SetWindowPos(window, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                    }
                    let _ = SetWindowPos(window, Some(front), 0, 0, 0, 0, flags);
                }
            }
        }
    }

    fn handle_of(value: isize) -> HWND {
        HWND(value as *mut std::ffi::c_void)
    }

    /// A menu open in one of AudioStrada's windows: that window's title (`None` for the main
    /// window) and the menu's areas on the desktop, in pixels (left, top, right, bottom).
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Menu {
        pub window: Option<String>,
        pub areas: Vec<[i32; 4]>,
    }

    /// Where AudioStrada's floating windows stand, as last set by [`own_windows`].
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Band {
        /// AudioStrada (or a plugin window) is in front: they float, topmost.
        Top,
        /// Just behind a window in front: one of AudioStrada's system dialogs (a file chooser,
        /// a message), or another application's window.
        Behind(isize),
    }

    thread_local! {
        static BAND: Cell<Option<Band>> = const { Cell::new(None) };
        /// Windows stepped aside for a menu: each one and the window showing the menu.
        static ASIDE: RefCell<Vec<(isize, isize)>> = const { RefCell::new(Vec::new()) };
    }

    /// Properties of the main window telling the plugin hosts about a menu open: the window
    /// showing it, and the menus' area packed in four 16-bit numbers (see [`pack`]).
    const MENU_WINDOW: PCWSTR = w!("AudioStradaMenuWindow");
    const MENU_AREA: PCWSTR = w!("AudioStradaMenuArea");

    pub(super) fn pack(area: [i32; 4]) -> isize {
        let part = |v: i32| i64::from(v.clamp(-32768, 32767) as i16 as u16);
        (part(area[0]) | part(area[1]) << 16 | part(area[2]) << 32 | part(area[3]) << 48) as isize
    }

    pub(super) fn unpack(value: isize) -> [i32; 4] {
        let v = value as i64;
        let part = |shift: u32| i32::from((v >> shift) as u16 as i16);
        [part(0), part(16), part(32), part(48)]
    }

    fn overlaps(a: &RECT, area: &[i32; 4]) -> bool {
        a.left < area[2] && area[0] < a.right && a.top < area[3] && area[1] < a.bottom
    }

    /// The window showing a menu of AudioStrada that `window` (of a plugin host) covers part
    /// of, as the main window tells it.
    fn menu_over(main: HWND, window: HWND) -> Option<HWND> {
        // SAFETY: plain Win32 calls; window properties are read across processes.
        let (owner, area) = unsafe { (GetPropW(main, MENU_WINDOW), GetPropW(main, MENU_AREA)) };
        if owner.is_invalid() {
            return None;
        }
        let owner = HWND(owner.0);
        (owner != window && overlaps(&rect_of(window), &unpack(area.0 as isize))).then_some(owner)
    }

    /// Keeps AudioStrada's windows as Reaper's floating windows are, on any screen. Every one
    /// is owned by the main window: never behind it, hidden with it when it is minimized, out
    /// of the taskbar and of Alt+Tab. While AudioStrada is in front, the floating ones (all but
    /// the detached panels titled `panels`: editors, the browser, dialogs, plugin windows) are
    /// topmost: a panel clicked never covers them, and the last one opened or clicked is in
    /// front of the others. Behind one of AudioStrada's system dialogs or another application
    /// in front, they leave the topmost band and stand just behind it. A window over one of
    /// the `menus` steps aside, behind the window showing it, until the menu closes; the plugin
    /// hosts read the menu on the main window (see [`menu_over`]). Cheap: call it at each
    /// frame, windows opened or moved since are placed on the next one. Returns the titles of
    /// the panels minimized (the main window not), which go back into it.
    pub fn own_windows(main: u64, panels: &[String], menus: &[Menu]) -> Vec<String> {
        let main = hwnd(main);
        let windows = winit_windows(main);
        let panel_windows: Vec<HWND> = windows
            .iter()
            .filter(|(_, title)| panels.contains(title))
            .map(|&(w, _)| w)
            .collect();
        let mut floating: Vec<HWND> = windows
            .iter()
            .map(|&(w, _)| w)
            .filter(|w| !panel_windows.contains(w))
            .collect();
        floating.extend(plugin_frames());
        for &(window, _) in &windows {
            // SAFETY: plain Win32 calls on one of this thread's windows.
            unsafe {
                // winit marks all its windows as application windows, which puts even owned
                // ones in Alt+Tab, and marks them again whenever their state changes
                // (maximized, resizable…): taken off at each frame.
                let extended = GetWindowLongPtrW(window, GWL_EXSTYLE);
                if extended & WS_EX_APPWINDOW.0 as isize != 0 {
                    SetWindowLongPtrW(
                        window,
                        GWL_EXSTYLE,
                        extended & !(WS_EX_APPWINDOW.0 as isize),
                    );
                }
            }
        }
        // SAFETY: plain Win32 call.
        let minimized = unsafe { IsIconic(main) }.as_bool();
        let order = z_order();
        let rank = |w: HWND| order.iter().position(|&o| o == w).unwrap_or(usize::MAX);
        // The windows showing a menu, and the windows over their menus.
        let showing: Vec<(HWND, &Menu)> = if minimized {
            Vec::new()
        } else {
            menus
                .iter()
                .filter_map(|m| {
                    let window = match &m.window {
                        None => Some(main),
                        Some(title) => windows.iter().find(|(_, t)| t == title).map(|&(w, _)| w),
                    }?;
                    Some((window, m))
                })
                .collect()
        };
        let before = ASIDE.with(|a| a.borrow().clone());
        let mut aside: Vec<(isize, isize)> = Vec::new();
        for &window in floating.iter().chain(&panel_windows) {
            let rect = rect_of(window);
            for &(owner, menu) in &showing {
                let key = (window.0 as isize, owner.0 as isize);
                if owner != window
                    && (before.contains(&key) || rank(window) < rank(owner))
                    && menu.areas.iter().any(|a| overlaps(&rect, a))
                {
                    aside.push(key);
                    break;
                }
            }
        }
        let is_aside = |w: HWND| aside.iter().any(|&(a, _)| a == w.0 as isize);
        // Owned by the main window, except while stepped aside behind it: an owned window
        // cannot go behind its owner.
        for &(window, _) in &windows {
            let behind_main = aside.contains(&(window.0 as isize, main.0 as isize));
            set_owner(window, if behind_main { HWND::default() } else { main });
        }
        for &(window, owner) in aside.iter().filter(|k| !before.contains(k)) {
            place_behind(handle_of(window), Some(handle_of(owner)));
        }
        // Back from aside: a panel goes back over the main window; floating windows go back
        // topmost below.
        for &(window, _) in before.iter().filter(|&&(w, _)| !is_aside(handle_of(w))) {
            let window = handle_of(window);
            if panel_windows.contains(&window) {
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                // SAFETY: plain Win32 call on one of this thread's windows.
                let _ = unsafe { SetWindowPos(window, Some(HWND_TOP), 0, 0, 0, 0, flags) };
            }
        }
        ASIDE.with(|a| a.borrow_mut().clone_from(&aside));
        publish_menu(main, &showing);
        // SAFETY: plain Win32 call.
        let front = unsafe { GetForegroundWindow() };
        let ours_in_front = front == main
            || windows.iter().any(|&(w, _)| w == front)
            || (class_of(front) == "AudioStradaPluginWindow" && ours(front));
        let band = if front.is_invalid() || minimized {
            // Between two windows (Alt+Tab, the desktop), or hidden: nothing to follow yet.
            BAND.with(|b| b.get())
        } else if ours_in_front {
            Some(Band::Top)
        } else {
            Some(Band::Behind(front.0 as isize))
        };
        let changed = BAND.with(|b| b.replace(band)) != band;
        // From the back to the front, so that they keep their order among themselves.
        let mut sorted: Vec<HWND> = floating.iter().copied().filter(|&w| !is_aside(w)).collect();
        sorted.sort_by_key(|&w| std::cmp::Reverse(rank(w)));
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        // Back from aside: behind the windows of the band, not over a window opened meanwhile
        // (About chosen in the menu it stepped aside for).
        let returning: Vec<HWND> = before
            .iter()
            .map(|&(w, _)| handle_of(w))
            .filter(|&w| floating.contains(&w) && !is_aside(w))
            .collect();
        match band {
            Some(Band::Top) => {
                for &window in sorted
                    .iter()
                    .filter(|&&w| !is_topmost(w) && !returning.contains(&w))
                {
                    place_behind(window, None);
                }
                if !returning.is_empty() {
                    let order = z_order();
                    let rank = |w: HWND| order.iter().position(|&o| o == w).unwrap_or(0);
                    let lowest = sorted
                        .iter()
                        .copied()
                        .filter(|&w| is_topmost(w) && !returning.contains(&w))
                        .max_by_key(|&w| rank(w));
                    for &window in &returning {
                        place_behind(window, None);
                        if let Some(lowest) = lowest {
                            place_behind(window, Some(lowest));
                        }
                    }
                }
            }
            Some(Band::Behind(front)) if changed => {
                let front = handle_of(front);
                for &window in &sorted {
                    if is_topmost(front) {
                        // A shell window (the taskbar, the Start menu): only out of the band.
                        // SAFETY: plain Win32 call on one of this thread's windows.
                        let _ = unsafe {
                            SetWindowPos(window, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags)
                        };
                    } else {
                        place_behind(window, Some(front));
                    }
                }
            }
            _ => {}
        }
        // Panels never float: what earlier versions left would cover other applications.
        for &window in panel_windows.iter().filter(|&&w| is_topmost(w)) {
            // SAFETY: plain Win32 call on one of this thread's windows.
            let _ = unsafe { SetWindowPos(window, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags) };
        }
        keep_maximized(&panel_windows, minimized, front);
        if minimized {
            return Vec::new();
        }
        windows
            .iter()
            // SAFETY: plain Win32 call on one of this thread's windows.
            .filter(|(w, _)| panel_windows.contains(w) && unsafe { IsIconic(*w) }.as_bool())
            .map(|(_, title)| title.clone())
            .collect()
    }

    thread_local! {
        /// The panels maximized before the main window was minimized, and when it came back.
        static ZOOMED: RefCell<(Vec<isize>, Option<std::time::Instant>, bool)> =
            const { RefCell::new((Vec::new(), None, false)) };
    }

    /// Panels maximized when the main window is minimized are maximized again when it comes
    /// back: Windows shows them again, but a maximized one on another screen came back at its
    /// normal size. The window in front before keeps the focus.
    fn keep_maximized(panels: &[HWND], minimized: bool, front: HWND) {
        ZOOMED.with(|z| {
            let (zoomed, restored, was_minimized) = &mut *z.borrow_mut();
            if minimized {
                *was_minimized = true;
                *restored = None;
                return;
            }
            if std::mem::take(was_minimized) {
                *restored = Some(std::time::Instant::now());
            }
            let settling =
                restored.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(1500));
            let mut fixed = false;
            for &panel in panels {
                // SAFETY: plain Win32 calls on one of this thread's windows.
                unsafe {
                    if settling
                        && zoomed.contains(&(panel.0 as isize))
                        && !IsZoomed(panel).as_bool()
                    {
                        let _ = ShowWindow(panel, SW_MAXIMIZE);
                        fixed = true;
                    }
                }
            }
            if fixed && !front.is_invalid() {
                // SAFETY: plain Win32 call; AudioStrada is in front.
                let _ = unsafe { SetForegroundWindow(front) };
            }
            if !settling {
                *zoomed = panels
                    .iter()
                    // SAFETY: plain Win32 calls on this thread's windows.
                    .filter(|&&p| unsafe { IsZoomed(p).as_bool() && IsWindowVisible(p).as_bool() })
                    .map(|p| p.0 as isize)
                    .collect();
            }
        });
    }

    #[cfg(test)]
    pub(super) fn z_order_for_tests() -> Vec<HWND> {
        z_order()
    }

    /// The menu the main window tells the plugin hosts about: the window showing it and its
    /// area.
    #[cfg(test)]
    pub(super) fn published_menu(main: HWND) -> Option<(HWND, [i32; 4])> {
        // SAFETY: plain Win32 calls.
        let (owner, area) = unsafe { (GetPropW(main, MENU_WINDOW), GetPropW(main, MENU_AREA)) };
        (!owner.is_invalid()).then(|| (HWND(owner.0), unpack(area.0 as isize)))
    }

    /// Tells the plugin hosts which window shows a menu, and where, through two properties of
    /// the main window; their windows over it step aside (see [`HostWindow::follow`]).
    fn publish_menu(main: HWND, showing: &[(HWND, &Menu)]) {
        let menu = showing.first().and_then(|&(window, menu)| {
            let mut areas = menu.areas.iter();
            let first = *areas.next()?;
            let union = areas.fold(first, |u, a| {
                [
                    u[0].min(a[0]),
                    u[1].min(a[1]),
                    u[2].max(a[2]),
                    u[3].max(a[3]),
                ]
            });
            Some((window, union))
        });
        // SAFETY: plain Win32 calls on the main window, of this thread.
        unsafe {
            match menu {
                Some((window, area)) => {
                    let _ = SetPropW(main, MENU_AREA, Some(HANDLE(pack(area) as *mut _)));
                    let _ = SetPropW(main, MENU_WINDOW, Some(HANDLE(window.0)));
                }
                None if !GetPropW(main, MENU_WINDOW).is_invalid() => {
                    let _ = RemovePropW(main, MENU_WINDOW);
                    let _ = RemovePropW(main, MENU_AREA);
                }
                None => {}
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
        /// Stepped aside, behind the window of AudioStrada showing a menu it covered.
        Aside(HWND),
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
                watch_keys(hwnd);
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
        /// AudioStrada's window, under the application in front, when another one is; behind
        /// AudioStrada's window showing a menu it covers part of, until the menu closes.
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
                let wanted = if let Some(menu) = menu_over(audiostrada, self.hwnd) {
                    Stand::Aside(menu)
                } else if front == audiostrada
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
                        // Back in the band from elsewhere (aside for a menu, behind a dialog
                        // or another application): behind the window of AudioStrada just
                        // activated, About opened from that menu say, not over it.
                        if self.stand != Stand::Top
                            && front != audiostrada
                            && main_process
                            && topmost(front)
                        {
                            let _ = SetWindowPos(self.hwnd, Some(front), 0, 0, 0, 0, flags);
                        }
                        self.refused = (!topmost(self.hwnd)).then(std::time::Instant::now);
                    }
                    // Already under the windows in front, above AudioStrada's: left there.
                    Stand::Away if self.stand != Stand::Top => {}
                    Stand::Away | Stand::Behind(_) | Stand::Aside(_) => {
                        // Out of the topmost band lands on top of the others: then just
                        // behind the window in front, unless it is topmost itself (a shell
                        // window), as following one would make this one topmost again.
                        let front = match wanted {
                            Stand::Aside(menu) => menu,
                            _ => front,
                        };
                        if matches!(wanted, Stand::Aside(_)) && topmost(front) {
                            // A floating window's menu: behind it, in the topmost band.
                            let _ = SetWindowPos(self.hwnd, Some(front), 0, 0, 0, 0, flags);
                        } else {
                            let _ =
                                SetWindowPos(self.hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                            if !topmost(front) {
                                let _ = SetWindowPos(self.hwnd, Some(front), 0, 0, 0, 0, flags);
                            }
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

        /// AudioStrada's keys pressed in the window since the last call (see `watch_keys`).
        pub fn take_keys(&mut self) -> Vec<(u32, u16)> {
            let frame = self.hwnd.0 as isize;
            PRESSED.with(|p| {
                let mut pressed = p.borrow_mut();
                let (mine, others): (Vec<_>, Vec<_>) =
                    pressed.drain(..).partition(|&(w, ..)| w == frame);
                *pressed = others;
                mine.into_iter().map(|(_, sym, mods)| (sym, mods)).collect()
            })
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
            forget_keys(self.hwnd);
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
    use std::ptr::NonNull;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSEvent, NSEventMask, NSEventModifierFlags,
        NSFloatingWindowLevel, NSNormalWindowLevel, NSRunningApplication, NSScreen, NSView,
        NSWindow, NSWindowLevel, NSWindowNumberListOptions, NSWindowOrderingMode,
        NSWindowStyleMask, NSWorkspace,
    };
    use objc2_core_foundation::{
        CFArray, CFDictionary, CFNumber, CFNumberType, CFRetained, CFString,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    use super::NativeHandle;

    /// The number of the window holding `view`, which every process can look for: AudioStrada's
    /// main window, for [`super::set_parent`].
    pub fn window_number(view: *mut std::ffi::c_void) -> u64 {
        // SAFETY: a live content view, used on the main thread.
        unsafe { view.cast::<NSView>().as_ref() }
            .and_then(|v| v.window())
            .map_or(0, |w| w.windowNumber() as u64)
    }

    /// AudioStrada's main window is on a screen: neither minimized nor hidden. True when it is
    /// not known.
    fn main_shown(mtm: MainThreadMarker) -> bool {
        let main = super::parent() as isize;
        if main == 0 {
            return true;
        }
        let options =
            NSWindowNumberListOptions::AllApplications | NSWindowNumberListOptions::AllSpaces;
        NSWindow::windowNumbersWithOptions(options, mtm)
            .is_none_or(|shown| shown.iter().any(|n| n.integerValue() == main))
    }

    /// The application's windows ordered out while the main window is minimized.
    static HIDDEN: Mutex<Vec<isize>> = Mutex::new(Vec::new());
    /// Windows stepped aside for a menu, by number.
    static ASIDE: Mutex<Vec<isize>> = Mutex::new(Vec::new());

    /// A window's frame from the top left of the main screen, in points: left, top, right,
    /// bottom, as AudioStrada's interface gives the menus' areas.
    fn top_left_frame(window: &NSWindow, mtm: MainThreadMarker) -> [f64; 4] {
        let screens = NSScreen::screens(mtm);
        let height = (screens.count() > 0).then(|| screens.objectAtIndex(0).frame().size.height);
        let f = window.frame();
        let top = height.unwrap_or(0.0) - (f.origin.y + f.size.height);
        [
            f.origin.x,
            top,
            f.origin.x + f.size.width,
            top + f.size.height,
        ]
    }

    fn overlaps(a: &[f64; 4], b: &[f64; 4]) -> bool {
        a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
    }

    /// True while AudioStrada is the active application: the application itself or one of its
    /// plugin hosts (the same executable), whose windows take the focus from it.
    fn audiostrada_active() -> bool {
        let Some(front) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return false;
        };
        let ours = NSRunningApplication::currentApplication().executableURL();
        ours.is_some() && front.executableURL() == ours
    }

    /// The levels of AudioStrada's windows while it is in use, from the bottom: detached panels
    /// float above the main window, the other floating windows (editors, browser, dialogs and
    /// plugin windows) in one band above them, so that clicking a panel never covers them and
    /// the last one clicked is in front; a window showing a menu or a dialog drawn inside it
    /// rises above them all. Levels hold across processes, so the plugin hosts' windows need
    /// no message.
    fn panel_level() -> NSWindowLevel {
        NSFloatingWindowLevel
    }

    fn band_level() -> NSWindowLevel {
        NSFloatingWindowLevel + 1
    }

    fn over_all_level() -> NSWindowLevel {
        NSFloatingWindowLevel + 2
    }

    /// A window kept above one rising above the plugin hosts' windows for its menu, in the same
    /// order as at rest.
    fn lifted_level(level: NSWindowLevel) -> NSWindowLevel {
        level + over_all_level() + 1 - panel_level()
    }

    /// The class of winit's windows, which key-value observing renames
    /// (`NSKVONotifying_WinitWindow`): an exact match missed every floating window.
    fn is_winit_window(class: &[u8]) -> bool {
        class.ends_with(b"WinitWindow")
    }

    /// At `level` while AudioStrada is in use, without taking the focus from it; at the normal
    /// level behind other applications.
    fn keep_above(window: &NSWindow, active: bool, level: NSWindowLevel) {
        let level = if active { level } else { NSNormalWindowLevel };
        if window.level() != level {
            window.setLevel(level);
        }
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut CFArray;
    }

    /// The frames on screen of the plugin hosts' windows (the processes of AudioStrada's
    /// executable but this one), from the top left of the main screen, in points.
    fn plugin_host_frames() -> Vec<[f64; 4]> {
        // On screen only, without the desktop's own windows.
        const OPTIONS: u32 = 1 | 16;
        // SAFETY: a plain query; the array returned is ours to release.
        let Some(list) = NonNull::new(unsafe { CGWindowListCopyWindowInfo(OPTIONS, 0) }) else {
            return Vec::new();
        };
        // SAFETY: a +1 reference from a Copy function.
        let list = unsafe { CFRetained::from_raw(list) };
        let ours = NSRunningApplication::currentApplication().executableURL();
        let me = std::process::id() as i64;
        let bounds_key = CFString::from_static_str("kCGWindowBounds");
        let number = |dict: &CFDictionary, key: &str| -> Option<f64> {
            let key = CFString::from_str(key);
            // SAFETY: the window list's values are CFNumbers under these keys.
            unsafe {
                let value = dict.value((&*key as *const CFString).cast());
                let value = value.cast::<CFNumber>().as_ref()?;
                let mut out = 0.0f64;
                value
                    .value(CFNumberType::Float64Type, (&mut out as *mut f64).cast())
                    .then_some(out)
            }
        };
        let mut hosts: Vec<(i64, bool)> = Vec::new();
        let mut frames = Vec::new();
        for i in 0..list.count() {
            // SAFETY: the window list holds dictionaries.
            let Some(dict) = (unsafe { list.value_at_index(i).cast::<CFDictionary>().as_ref() })
            else {
                continue;
            };
            let Some(pid) = number(dict, "kCGWindowOwnerPID").map(|p| p as i64) else {
                continue;
            };
            if pid == me {
                continue;
            }
            let host = match hosts.iter().find(|(p, _)| *p == pid) {
                Some(&(_, host)) => host,
                None => {
                    let host = ours.is_some()
                        && NSRunningApplication::runningApplicationWithProcessIdentifier(
                            pid as libc::pid_t,
                        )
                        .and_then(|app| app.executableURL())
                            == ours;
                    hosts.push((pid, host));
                    host
                }
            };
            if !host {
                continue;
            }
            // SAFETY: the bounds are a dictionary of CFNumbers.
            let Some(bounds) = (unsafe {
                dict.value((&*bounds_key as *const CFString).cast())
                    .cast::<CFDictionary>()
                    .as_ref()
            }) else {
                continue;
            };
            if let (Some(x), Some(y), Some(w), Some(h)) = (
                number(bounds, "X"),
                number(bounds, "Y"),
                number(bounds, "Width"),
                number(bounds, "Height"),
            ) {
                frames.push([x, y, x + w, y + h]);
            }
        }
        frames
    }

    /// Windows lifted above the main window showing a menu, by number.
    static LIFTED: Mutex<Vec<isize>> = Mutex::new(Vec::new());

    /// Keeps the application's other windows (winit's: detached panels, titled `panels`, and
    /// the others) above its main window, given by its content view, as [`HostWindow`] does
    /// for plugin windows, at the levels above. While the main window shows a dialog
    /// (`covered`), it rises above them all, plugin windows of other processes included. A
    /// window over one of the `menus` (the title of the window showing it, `None` for the main
    /// one, and its areas) drops behind that window until it closes; a plugin host's window
    /// cannot be told, so the window showing the menu rises above it instead, the other
    /// windows of this process staying where they are.
    pub fn keep_windows_above(
        main: *mut std::ffi::c_void,
        panels: &[String],
        covered: bool,
        menus: &[(Option<String>, Vec<[f64; 4]>)],
    ) -> Vec<String> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Vec::new();
        };
        // SAFETY: the main window's live content view, used on the main thread.
        let main = unsafe { main.cast::<NSView>().as_ref() }.and_then(|v| v.window());
        let active = audiostrada_active();
        // Minimized with the main window, as on Windows: left on the screen, they would keep
        // AudioStrada in front, and neither the Dock nor the app switcher would bring it back.
        let minimized = main.as_ref().is_some_and(|m| m.isMiniaturized());
        let mut hidden = HIDDEN.lock().unwrap_or_else(|e| e.into_inner());
        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        // Where each of the application's windows rests while AudioStrada is in use.
        let resting = |window: &NSWindow| -> NSWindowLevel {
            if Some(window) == main.as_deref() {
                NSNormalWindowLevel
            } else if panels.contains(&window.title().to_string()) {
                panel_level()
            } else {
                band_level()
            }
        };
        // The windows showing a menu, by number, with the menus' areas and their own level.
        let showing: Vec<(isize, NSWindowLevel, &Vec<[f64; 4]>)> = menus
            .iter()
            .filter_map(|(title, areas)| {
                let owner = match title {
                    None => main.clone()?,
                    Some(t) => windows.iter().find(|w| w.title().to_string() == *t)?,
                };
                Some((owner.windowNumber(), resting(&owner), areas))
            })
            .collect();
        // The window showing a menu that a plugin host's window covers.
        let rising = if showing.is_empty() {
            None
        } else {
            let hosts = plugin_host_frames();
            showing
                .iter()
                .find(|(_, _, areas)| areas.iter().any(|a| hosts.iter().any(|h| overlaps(a, h))))
                .map(|&(owner, ..)| owner)
        };
        let main_number = main.as_ref().map(|m| m.windowNumber());
        if let Some(main) = &main {
            let level = if active && (covered || rising == main_number) {
                over_all_level()
            } else {
                NSNormalWindowLevel
            };
            if main.level() != level {
                main.setLevel(level);
            }
        }
        let mut aside = ASIDE.lock().unwrap_or_else(|e| e.into_inner());
        let mut lifted = LIFTED.lock().unwrap_or_else(|e| e.into_inner());
        let mut still = Vec::new();
        let mut still_lifted = Vec::new();
        let mut back = false;
        // Panels minimized: they go back into the main window. egui does not follow the
        // minimized state on macOS while running.
        let mut docked = Vec::new();
        for window in windows {
            if Some(&window) == main.as_ref() || !is_winit_window(window.class().name().to_bytes())
            {
                continue;
            }
            let number = window.windowNumber();
            if minimized {
                if window.isVisible() {
                    window.orderOut(None);
                    hidden.push(number);
                }
                continue;
            }
            let title = window.title().to_string();
            if panels.contains(&title) && window.isMiniaturized() {
                docked.push(title);
                continue;
            }
            let level = resting(&window);
            let frame = top_left_frame(&window, mtm);
            let over = showing.iter().find(|(owner, _, areas)| {
                *owner != number && areas.iter().any(|a| overlaps(a, &frame))
            });
            // Behind the window showing the menu, at its level: no lower, or it would go
            // behind the main window too.
            if let Some(&(owner, owner_level, _)) = over {
                keep_above(&window, active, level.min(owner_level));
                if !aside.contains(&number) {
                    window.orderWindow_relativeTo(NSWindowOrderingMode::Below, owner);
                }
                still.push(number);
                continue;
            }
            // The window showing the menu rises above the plugin hosts' windows; the others
            // of the band stay above it, and the panels too when it is the main window, in the
            // same order.
            let level = match rising {
                Some(r) if r == number => over_all_level(),
                Some(r) if Some(r) == main_number || level == band_level() => {
                    still_lifted.push(number);
                    lifted_level(level)
                }
                _ => level,
            };
            keep_above(&window, active, level);
            // Back from aside: behind the other floating windows, never over one opened
            // from the menu (About).
            if aside.contains(&number) {
                window.orderBack(None);
                back = true;
            }
            back |= lifted.contains(&number) && !still_lifted.contains(&number);
            if hidden.contains(&number) {
                window.orderFront(None);
            }
        }
        *aside = still;
        *lifted = still_lifted;
        // The window in use comes back to the front of the band.
        if back
            && let Some(key) = app.keyWindow()
            && Some(&key) != main.as_ref()
            && !aside.contains(&key.windowNumber())
        {
            key.orderFront(None);
        }
        if !minimized {
            hidden.clear();
        }
        docked
    }

    /// The numbers of this process's plugin windows, and AudioStrada's keys pressed in them:
    /// window, keysym and modifiers.
    static FRAMES: Mutex<Vec<isize>> = Mutex::new(Vec::new());
    static PRESSED: Mutex<Vec<(isize, u32, u16)>> = Mutex::new(Vec::new());

    thread_local! {
        /// The monitor of the application's key presses, kept for the process's life.
        static MONITOR: std::cell::RefCell<Option<objc2::rc::Retained<objc2::runtime::AnyObject>>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Takes AudioStrada's keys (the transport's shortcuts) from the key presses of this
    /// process's plugin windows, before the plugin sees them: Space plays whatever plugin
    /// window is in front, as on the other systems.
    fn watch_keys(window: isize) {
        FRAMES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(window);
        MONITOR.with(|monitor| {
            if monitor.borrow().is_some() {
                return;
            }
            let block = block2::RcBlock::new(|event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
                // SAFETY: AppKit hands a live event to the monitor.
                if take_key(unsafe { event.as_ref() }) {
                    std::ptr::null_mut()
                } else {
                    event.as_ptr()
                }
            });
            // SAFETY: the block returns the event it was given or null, as asked.
            let added = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
            };
            *monitor.borrow_mut() = added;
        });
    }

    fn forget_keys(window: isize) {
        FRAMES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|&w| w != window);
        PRESSED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|&(w, ..)| w != window);
    }

    /// Keeps a key pressed in one of our plugin windows when it is one of AudioStrada's; true
    /// when it was taken, the plugin then getting nothing.
    fn take_key(event: &NSEvent) -> bool {
        let window = event.windowNumber();
        if !FRAMES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&window)
        {
            return false;
        }
        let flags = event.modifierFlags();
        if flags.contains(NSEventModifierFlags::Control) {
            return false;
        }
        let mods = u16::from(flags.contains(NSEventModifierFlags::Shift))
            | u16::from(flags.contains(NSEventModifierFlags::Command)) << 2
            | u16::from(flags.contains(NSEventModifierFlags::Option)) << 3;
        let Some(sym) = keysym(event) else {
            return false;
        };
        if !super::grabbed_keys().contains(&(sym, mods)) {
            return false;
        }
        // A key held down repeats: one press only.
        if !event.isARepeat() {
            PRESSED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((window, sym, mods));
        }
        true
    }

    /// The X keysym of a key press, among those AudioStrada's shortcuts use.
    fn keysym(event: &NSEvent) -> Option<u32> {
        Some(match event.keyCode() {
            49 => 0x20,
            36 | 76 => 0xff0d,
            53 => 0xff1b,
            48 => 0xff09,
            51 => 0xff08,
            117 => 0xffff,
            114 => 0xff63,
            115 => 0xff50,
            119 => 0xff57,
            116 => 0xff55,
            121 => 0xff56,
            123 => 0xff51,
            126 => 0xff52,
            124 => 0xff53,
            125 => 0xff54,
            code => {
                // F1 to F12.
                const F: [u16; 12] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];
                if let Some(k) = F.iter().position(|&f| f == code) {
                    return Some(0xffbe + k as u32);
                }
                let text = event.charactersIgnoringModifiers()?.to_string();
                let c = text.chars().next()?.to_ascii_lowercase();
                c.is_ascii_graphic().then_some(c as u32)?
            }
        })
    }

    /// An AppKit window whose content view holds the plugin's editor. Its process cannot be
    /// owned by AudioStrada's window (another process): it floats above AudioStrada's windows
    /// while AudioStrada is in use instead.
    pub struct HostWindow {
        window: Retained<NSWindow>,
        mtm: MainThreadMarker,
        /// Ordered out while AudioStrada's main window is minimized or hidden.
        hidden: bool,
        checked: Instant,
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
            keep_above(&window, true, band_level());
            watch_keys(window.windowNumber());
            let mut host = Self {
                window,
                mtm,
                hidden: false,
                checked: Instant::now(),
            };
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
            self.hidden = false;
            self.window.makeKeyAndOrderFront(None);
            // `activate` only exists from macOS 14 on.
            #[allow(deprecated)]
            NSApplication::sharedApplication(self.mtm).activateIgnoringOtherApps(true);
        }

        /// AudioStrada's keys pressed in the window since the last call (see `watch_keys`).
        pub fn take_keys(&mut self) -> Vec<(u32, u16)> {
            let window = self.window.windowNumber();
            let mut pressed = PRESSED.lock().unwrap_or_else(|e| e.into_inner());
            let (mine, others): (Vec<_>, Vec<_>) =
                pressed.drain(..).partition(|&(w, ..)| w == window);
            *pressed = others;
            mine.into_iter().map(|(_, sym, mods)| (sym, mods)).collect()
        }

        /// True once the user closed the window (ordered out, not merely minimized). Hides it
        /// while AudioStrada's main window is minimized or hidden, and shows it again after.
        pub fn poll(&mut self) -> bool {
            keep_above(&self.window, audiostrada_active(), band_level());
            if self.checked.elapsed() >= Duration::from_millis(200) {
                self.checked = Instant::now();
                let shown = main_shown(self.mtm);
                if !shown && !self.hidden && self.window.isVisible() {
                    self.window.orderOut(None);
                    self.hidden = true;
                } else if shown && self.hidden {
                    self.window.orderFront(None);
                    self.hidden = false;
                }
            }
            !self.hidden && !self.window.isVisible() && !self.window.isMiniaturized()
        }
    }

    impl Drop for HostWindow {
        fn drop(&mut self) {
            forget_keys(self.window.windowNumber());
            self.window.close();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn winit_windows_are_found_under_key_value_observing() {
            assert!(is_winit_window(b"WinitWindow"));
            assert!(is_winit_window(b"NSKVONotifying_WinitWindow"));
            assert!(!is_winit_window(b"NSKVONotifying_TUINSWindow"));
            assert!(!is_winit_window(b"NSWindow"));
        }

        #[test]
        fn windows_lifted_for_a_menu_keep_their_order_above_the_window_showing_it() {
            assert!(panel_level() < band_level() && band_level() < over_all_level());
            assert!(lifted_level(panel_level()) > over_all_level());
            assert!(lifted_level(band_level()) > lifted_level(panel_level()));
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
#[cfg(all(unix, not(target_os = "macos")))]
pub use imp::{
    Followed, follow_main, publish_menu, raise_parent, raise_titled, window_back_from_aside,
};
#[cfg(windows)]
pub use imp::{Menu, own_windows};
#[cfg(target_os = "macos")]
pub use imp::{keep_windows_above, window_number};

impl std::fmt::Debug for HostWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostWindow")
            .field("handle", &self.handle())
            .finish()
    }
}

/// The screens' rectangles `[x, y, width, height]` in the desktop's coordinates: pixels on
/// Windows, points from the top left of the main screen on macOS. Empty elsewhere (X11 lists
/// them in the interface).
pub fn monitors() -> Vec<[f64; 4]> {
    monitors_of_system()
}

#[cfg(windows)]
fn monitors_of_system() -> Vec<[f64; 4]> {
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::core::BOOL;

    unsafe extern "system" fn add(_: HMONITOR, _: HDC, rect: *mut RECT, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the vector below, alive during the enumeration; `rect` is the
        // monitor's rectangle or null.
        unsafe {
            if let (Some(list), Some(r)) = ((list.0 as *mut Vec<[f64; 4]>).as_mut(), rect.as_ref())
            {
                list.push([
                    f64::from(r.left),
                    f64::from(r.top),
                    f64::from(r.right - r.left),
                    f64::from(r.bottom - r.top),
                ]);
            }
        }
        true.into()
    }
    let mut list: Vec<[f64; 4]> = Vec::new();
    // SAFETY: the callback only writes into `list`, which outlives the call.
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(add),
            LPARAM(&mut list as *mut Vec<[f64; 4]> as isize),
        )
    };
    list
}

#[cfg(target_os = "macos")]
fn monitors_of_system() -> Vec<[f64; 4]> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let screens = NSScreen::screens(mtm);
    let frames: Vec<_> = (0..screens.count())
        .map(|i| screens.objectAtIndex(i).frame())
        .collect();
    // AppKit counts from the bottom left of the main screen, upwards.
    let Some(main) = frames.first().map(|f| f.size.height) else {
        return Vec::new();
    };
    frames
        .iter()
        .map(|f| {
            [
                f.origin.x,
                main - (f.origin.y + f.size.height),
                f.size.width,
                f.size.height,
            ]
        })
        .collect()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn monitors_of_system() -> Vec<[f64; 4]> {
    Vec::new()
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

    /// A plugin window stays out of the taskbar and Alt+Tab, hides while AudioStrada's main
    /// window is minimized (the window manager says so with `_NET_WM_STATE_HIDDEN`) and comes
    /// back with it. Needs AUDIOSTRADA_TEST_X11_KEYS and a display (Xvfb).
    #[test]
    fn plugin_windows_hide_while_the_main_window_is_minimized() {
        use x11rb::protocol::xproto::{AtomEnum, MapState, PropMode};
        if std::env::var_os("AUDIOSTRADA_TEST_X11_KEYS").is_none() {
            eprintln!("AUDIOSTRADA_TEST_X11_KEYS not set, skipping");
            return;
        }
        let (c, screen) = x11rb::connect(None).unwrap();
        let root = c.setup().roots[screen].root;
        let main = c.generate_id().unwrap();
        c.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            main,
            root,
            0,
            0,
            300,
            200,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .unwrap();
        c.map_window(main).unwrap();
        c.sync().unwrap();
        set_parent(u64::from(main));
        let mut host = HostWindow::new("hidden", 200, 100).unwrap();
        let NativeHandle::X11(window) = host.handle() else {
            unreachable!()
        };
        let atom = |n: &[u8]| c.intern_atom(false, n).unwrap().reply().unwrap().atom;
        let (state, hidden) = (atom(b"_NET_WM_STATE"), atom(b"_NET_WM_STATE_HIDDEN"));
        let states = c
            .get_property(false, window, state, AtomEnum::ATOM, 0, 16)
            .unwrap()
            .reply()
            .unwrap();
        let states: Vec<u32> = states.value32().unwrap().collect();
        assert!(states.contains(&atom(b"_NET_WM_STATE_SKIP_TASKBAR")));
        let mapped = || {
            c.get_window_attributes(window)
                .unwrap()
                .reply()
                .unwrap()
                .map_state
                == MapState::VIEWABLE
        };
        let settle = |host: &mut HostWindow| {
            std::thread::sleep(std::time::Duration::from_millis(250));
            host.poll();
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert!(mapped());
        c.change_property32(PropMode::REPLACE, main, state, AtomEnum::ATOM, &[hidden])
            .unwrap();
        c.sync().unwrap();
        settle(&mut host);
        assert!(!mapped(), "hidden with the main window");
        c.change_property32(PropMode::REPLACE, main, state, AtomEnum::ATOM, &[])
            .unwrap();
        c.sync().unwrap();
        settle(&mut host);
        assert!(mapped(), "back with it");
        set_parent(0);
    }

    /// AudioStrada's other windows are kept transient for the main one, but those stepping
    /// behind it for a menu; panels minimized are told; AudioStrada is active while one of its
    /// windows is. The test stands in for the window manager's lists. Needs
    /// AUDIOSTRADA_TEST_X11_KEYS and a display (Xvfb).
    #[test]
    fn floating_windows_stay_transient_for_the_main_window_but_while_stepping_behind() {
        use x11rb::protocol::xproto::{AtomEnum, PropMode};
        if std::env::var_os("AUDIOSTRADA_TEST_X11_KEYS").is_none() {
            eprintln!("AUDIOSTRADA_TEST_X11_KEYS not set, skipping");
            return;
        }
        let (c, screen) = x11rb::connect(None).unwrap();
        let root = c.setup().roots[screen].root;
        let atom = |n: &[u8]| c.intern_atom(false, n).unwrap().reply().unwrap().atom;
        let (pid, name, utf8) = (
            atom(b"_NET_WM_PID"),
            atom(b"_NET_WM_NAME"),
            atom(b"UTF8_STRING"),
        );
        let window = |title: &str, ours: bool| {
            let w = c.generate_id().unwrap();
            c.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                w,
                root,
                0,
                0,
                100,
                100,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .unwrap();
            if ours {
                c.change_property32(
                    PropMode::REPLACE,
                    w,
                    pid,
                    AtomEnum::CARDINAL,
                    &[std::process::id()],
                )
                .unwrap();
            }
            c.change_property8(PropMode::REPLACE, w, name, utf8, title.as_bytes())
                .unwrap();
            w
        };
        let (main, mixer, editor) = (
            window("Main", true),
            window("Mixer", true),
            window("Editor", true),
        );
        let other = window("Other", false);
        let set = |w: u32, property: &[u8], kind: AtomEnum, values: &[u32]| {
            c.change_property32(PropMode::REPLACE, w, atom(property), kind, values)
                .unwrap();
            c.sync().unwrap();
        };
        set(
            root,
            b"_NET_CLIENT_LIST",
            AtomEnum::WINDOW,
            &[main, mixer, editor, other],
        );
        set(root, b"_NET_ACTIVE_WINDOW", AtomEnum::WINDOW, &[editor]);
        let transient = |w: u32| {
            c.get_property(false, w, AtomEnum::WM_TRANSIENT_FOR, AtomEnum::WINDOW, 0, 1)
                .unwrap()
                .reply()
                .unwrap()
                .value32()
                .and_then(|mut v| v.next())
        };
        let panels = ["Mixer".to_owned()];
        let followed = follow_main(u64::from(main), &[], &panels);
        c.sync().unwrap();
        assert!(followed.active, "one of its windows is active");
        assert_eq!(transient(mixer), Some(main));
        assert_eq!(transient(editor), Some(main));
        assert_eq!(
            transient(other),
            None,
            "another application's window is left alone"
        );
        follow_main(u64::from(main), &["Editor".to_owned()], &panels);
        c.sync().unwrap();
        assert_eq!(transient(editor), None, "stepping behind the main window");
        assert_eq!(transient(mixer), Some(main));
        follow_main(u64::from(main), &[], &panels);
        c.sync().unwrap();
        assert_eq!(transient(editor), Some(main), "back above it");
        let hidden = atom(b"_NET_WM_STATE_HIDDEN");
        set(mixer, b"_NET_WM_STATE", AtomEnum::ATOM, &[hidden]);
        set(root, b"_NET_ACTIVE_WINDOW", AtomEnum::WINDOW, &[other]);
        std::thread::sleep(std::time::Duration::from_millis(150));
        let followed = follow_main(u64::from(main), &[], &panels);
        assert!(!followed.active, "another application is active");
        assert_eq!(followed.minimized, panels, "a panel minimized docks back");
        set(main, b"_NET_WM_STATE", AtomEnum::ATOM, &[hidden]);
        std::thread::sleep(std::time::Duration::from_millis(150));
        let followed = follow_main(u64::from(main), &[], &panels);
        assert!(
            followed.minimized.is_empty(),
            "hidden with the main window: it stays"
        );
        for w in [main, mixer, editor, other] {
            c.destroy_window(w).unwrap();
        }
        c.delete_property(root, atom(b"_NET_CLIENT_LIST")).unwrap();
        c.delete_property(root, atom(b"_NET_ACTIVE_WINDOW"))
            .unwrap();
        c.sync().unwrap();
    }

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
        GetForegroundWindow, GetWindowLongPtrW, HWND_TOP, HWND_TOPMOST, IsZoomed, RegisterClassW,
        SW_MINIMIZE, SW_RESTORE, SW_SHOWMAXIMIZED, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetForegroundWindow, SetWindowPos, ShowWindow, WNDCLASSW, WS_EX_APPWINDOW,
        WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, PostMessageW, TranslateMessage, WM_KEYDOWN,
        WS_CHILD, WS_VISIBLE,
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

    fn z_rank(window: HWND) -> usize {
        imp::z_order_for_tests()
            .iter()
            .position(|&w| w == window)
            .unwrap()
    }

    fn topmost(window: HWND) -> bool {
        // SAFETY: plain Win32 call.
        unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0 }
    }

    fn destroy(windows: &[HWND]) {
        for &window in windows {
            // SAFETY: windows of this test, destroyed once.
            let _ = unsafe { DestroyWindow(window) };
        }
    }

    /// Every window is owned by the main window, out of Alt+Tab; a panel never floats; while
    /// AudioStrada is in front the others float above the panels, the last one put in front
    /// staying in front.
    #[test]
    fn floating_windows_are_owned_by_the_main_window_and_float_above_panels() {
        let main = window("Main — float", 0);
        let mixer = window("Mixer — float", 2000);
        let browser = window("Browser — float", 2100);
        let editor = window("Editor — float", 2200);
        let (main_id, panels) = (main.0 as u64, vec!["Mixer — float".to_owned()]);

        imp::own_windows(main_id, &panels, &[]);
        for w in [mixer, browser, editor] {
            assert_eq!(owner(w), main.0 as isize, "owned by the main window");
            // SAFETY: plain Win32 call.
            let extended = unsafe { GetWindowLongPtrW(w, GWL_EXSTYLE) };
            assert_eq!(extended & WS_EX_APPWINDOW.0 as isize, 0, "out of Alt+Tab");
        }
        // SAFETY: plain Win32 call; the test runner may refuse it the foreground.
        let _ = unsafe { SetForegroundWindow(main) };
        imp::own_windows(main_id, &panels, &[]);
        assert!(!topmost(mixer), "a panel never floats");
        // SAFETY: plain Win32 call.
        if unsafe { GetForegroundWindow() } == main {
            assert!(
                topmost(browser) && topmost(editor),
                "floating windows float"
            );
            assert!(z_rank(browser) < z_rank(mixer), "above the mixer");
            // The browser opened again, then the mixer clicked: the browser stays in front.
            // SAFETY: plain Win32 calls on windows of this test.
            unsafe {
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                SetWindowPos(browser, Some(HWND_TOPMOST), 0, 0, 0, 0, flags).unwrap();
                SetWindowPos(mixer, Some(HWND_TOP), 0, 0, 0, 0, flags).unwrap();
            }
            imp::own_windows(main_id, &panels, &[]);
            assert!(
                z_rank(browser) < z_rank(editor),
                "the last one in front stays there"
            );
            assert!(
                z_rank(editor) < z_rank(mixer),
                "the mixer clicked covers neither"
            );
        } else {
            eprintln!("not in front: the topmost band is not checked");
        }
        destroy(&[editor, browser, mixer, main]);
    }

    /// A window over a menu of the main window steps aside behind it, its other windows stay
    /// where they are; it comes back, owned again, once the menu closes. The plugin hosts read
    /// the menu on the main window.
    #[test]
    fn windows_over_a_menu_step_aside_until_it_closes() {
        let main = window("Main — menu", 0);
        let over = window("Over — menu", 100);
        let away = window("Away — menu", 2100);
        let (main_id, panels) = (main.0 as u64, Vec::new());
        // SAFETY: plain Win32 calls on windows of this test.
        unsafe {
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            SetWindowPos(over, Some(HWND_TOP), 0, 0, 0, 0, flags).unwrap();
            SetWindowPos(away, Some(HWND_TOP), 0, 0, 0, 0, flags).unwrap();
        }
        imp::own_windows(main_id, &panels, &[]);
        assert!(z_rank(over) < z_rank(main));
        let menu = imp::Menu {
            window: None,
            areas: vec![[120, 20, 320, 400]],
        };
        imp::own_windows(main_id, &panels, std::slice::from_ref(&menu));
        assert!(z_rank(over) > z_rank(main), "behind the main window");
        assert_eq!(owner(over), 0, "which no longer owns it meanwhile");
        assert!(z_rank(away) < z_rank(main), "elsewhere: left in front");
        assert_eq!(owner(away), main.0 as isize);
        assert_eq!(
            imp::published_menu(main),
            Some((main, [120, 20, 320, 400])),
            "told to the plugin hosts"
        );
        imp::own_windows(main_id, &panels, &[]);
        assert_eq!(owner(over), main.0 as isize, "back, owned again");
        assert!(z_rank(over) < z_rank(main), "in front of the main window");
        assert_eq!(imp::published_menu(main), None);
        destroy(&[away, over, main]);
    }

    /// A panel maximized on its screen when the main window is minimized is maximized again
    /// when the main window comes back.
    #[test]
    fn a_maximized_panel_comes_back_maximized_with_the_main_window() {
        let main = window("Main — zoom", 0);
        let mixer = window("Mixer — zoom", 2000);
        let (main_id, panels) = (main.0 as u64, vec!["Mixer — zoom".to_owned()]);
        // SAFETY: plain Win32 calls on windows of this test.
        unsafe {
            imp::own_windows(main_id, &panels, &[]);
            let _ = ShowWindow(mixer, SW_SHOWMAXIMIZED);
            imp::own_windows(main_id, &panels, &[]);
            let _ = ShowWindow(main, SW_MINIMIZE);
            imp::own_windows(main_id, &panels, &[]);
            let _ = ShowWindow(main, SW_RESTORE);
            // What was seen: back at its normal size.
            let _ = ShowWindow(mixer, SW_SHOWNOACTIVATE);
            let _ = ShowWindow(mixer, SW_RESTORE);
            imp::own_windows(main_id, &panels, &[]);
            assert!(IsZoomed(mixer).as_bool(), "maximized again");
        }
        destroy(&[mixer, main]);
    }

    /// A panel minimized is told, to go back into the main window; panels hidden with the
    /// main window minimized are not.
    #[test]
    fn a_minimized_panel_is_told_to_dock_back() {
        let main = window("Main — dock", 0);
        let mixer = window("Mixer — dock", 2000);
        let (main_id, panels) = (main.0 as u64, vec!["Mixer — dock".to_owned()]);
        assert!(imp::own_windows(main_id, &panels, &[]).is_empty());
        // SAFETY: plain Win32 calls on windows of this test.
        unsafe {
            let _ = ShowWindow(mixer, SW_MINIMIZE);
        }
        assert_eq!(imp::own_windows(main_id, &panels, &[]), panels);
        // SAFETY: as above.
        unsafe {
            let _ = ShowWindow(mixer, SW_RESTORE);
            let _ = ShowWindow(main, SW_MINIMIZE);
        }
        assert!(
            imp::own_windows(main_id, &panels, &[]).is_empty(),
            "hidden with the main window"
        );
        destroy(&[mixer, main]);
    }

    #[test]
    fn a_menu_area_crosses_processes_whole() {
        for area in [
            [0, 0, 100, 100],
            [-1920, -200, -10, 1080],
            [3840, 0, 7680, 2160],
        ] {
            assert_eq!(imp::unpack(imp::pack(area)), area);
        }
    }

    static KEYS_SEEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    unsafe extern "system" fn plugin_procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if m == WM_KEYDOWN {
            KEYS_SEEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // SAFETY: default handling, with what Windows handed us.
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    #[test]
    fn plugin_windows_hand_back_audiostradas_keys() {
        set_grabbed_keys(vec![(0x20, 0)]);
        let mut host = HostWindow::new("keys", 300, 200).unwrap();
        let NativeHandle::Win32(frame) = host.handle() else {
            unreachable!()
        };
        // SAFETY: plain Win32 calls with valid arguments, on this test's thread.
        unsafe {
            let instance = GetModuleHandleW(None).unwrap().into();
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(plugin_procedure),
                hInstance: instance,
                lpszClassName: w!("TestPluginView"),
                ..Default::default()
            });
            // The plugin: a child window of the frame, listening to keys.
            let view = CreateWindowExW(
                Default::default(),
                w!("TestPluginView"),
                w!(""),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                300,
                200,
                Some(HWND(frame)),
                None,
                Some(instance),
                None,
            )
            .unwrap();
            let pump = || {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            };
            // Space (no modifier held), then Space held down (a repeat), then a key of the
            // plugin's own.
            PostMessageW(Some(view), WM_KEYDOWN, WPARAM(0x20), LPARAM(1)).unwrap();
            PostMessageW(Some(view), WM_KEYDOWN, WPARAM(0x20), LPARAM(1 | (1 << 30))).unwrap();
            PostMessageW(Some(view), WM_KEYDOWN, WPARAM(0x41), LPARAM(1)).unwrap();
            pump();
            assert_eq!(
                host.take_keys(),
                vec![(0x20, 0)],
                "AudioStrada gets Space once"
            );
            assert_eq!(
                KEYS_SEEN.load(std::sync::atomic::Ordering::Relaxed),
                1,
                "the plugin gets its own key only"
            );
            assert!(host.take_keys().is_empty());
            let _ = DestroyWindow(view);
        }
    }
}
