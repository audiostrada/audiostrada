//! LV2 plugin windows: X11 UIs (Windows UIs on Windows) embedded in a host window, talking to
//! the plugin through its control ports with the float protocol.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{Feature, Host, OptionEntry, Plugin, PortInfo, PortKind, UridMap, UridUnmap};
use crate::window::{HostWindow, NativeHandle};

#[cfg(windows)]
const UI_CLASS: &str = "http://lv2plug.in/ns/extensions/ui#WindowsUI";
#[cfg(not(windows))]
const UI_CLASS: &str = "http://lv2plug.in/ns/extensions/ui#X11UI";
const IDLE_INTERFACE: &CStr = c"http://lv2plug.in/ns/extensions/ui#idleInterface";

type WriteFunction = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void);

#[repr(C)]
struct UiDescriptor {
    uri: *const c_char,
    instantiate: Option<
        unsafe extern "C" fn(
            *const UiDescriptor,
            *const c_char,
            *const c_char,
            WriteFunction,
            *mut c_void,
            *mut *mut c_void,
            *const *const Feature,
        ) -> *mut c_void,
    >,
    cleanup: Option<unsafe extern "C" fn(*mut c_void)>,
    port_event: Option<unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void)>,
    extension_data: Option<unsafe extern "C" fn(*const c_char) -> *const c_void>,
}

#[repr(C)]
struct IdleInterface {
    idle: unsafe extern "C" fn(*mut c_void) -> i32,
}

#[repr(C)]
struct Resize {
    handle: *mut c_void,
    ui_resize: unsafe extern "C" fn(*mut c_void, i32, i32) -> i32,
}

/// An LV2 UI this host can show: its URI, bundle directory and library.
#[derive(Clone, Debug)]
pub(super) struct UiChoice {
    uri: CString,
    bundle: CString,
    binary: PathBuf,
}

fn parse_file_uri(host: &Host, uri: *const c_char) -> Option<String> {
    // SAFETY: plain lilv call; the parsed path is freed with lilv_free.
    unsafe {
        let path = (host.lilv.lilv_file_uri_parse)(uri, std::ptr::null_mut());
        if path.is_null() {
            return None;
        }
        let out = CStr::from_ptr(path).to_string_lossy().into_owned();
        (host.lilv.lilv_free)(path as *mut c_void);
        Some(out)
    }
}

/// The first UI of `plugin` of the kind this system embeds.
pub(super) fn find(host: &mut Host, plugin: *const Plugin) -> Option<UiChoice> {
    let class = host.node(UI_CLASS);
    // SAFETY: standard lilv iteration; the collection is freed at the end.
    unsafe {
        let uis = (host.lilv.lilv_plugin_get_uis)(plugin);
        if uis.is_null() {
            return None;
        }
        let mut found = None;
        let mut it = (host.lilv.lilv_uis_begin)(uis);
        while !(host.lilv.lilv_uis_is_end)(uis, it) {
            let ui = (host.lilv.lilv_uis_get)(uis, it);
            if (host.lilv.lilv_ui_is_a)(ui, class) {
                let uri = (host.lilv.lilv_ui_get_uri)(ui);
                (host.lilv.lilv_world_load_resource)(host.world, uri);
                let bundle = parse_file_uri(
                    host,
                    (host.lilv.lilv_node_as_uri)((host.lilv.lilv_ui_get_bundle_uri)(ui)),
                );
                let binary = parse_file_uri(
                    host,
                    (host.lilv.lilv_node_as_uri)((host.lilv.lilv_ui_get_binary_uri)(ui)),
                );
                if let (Some(bundle), Some(binary)) = (bundle, binary) {
                    let bundle = if bundle.ends_with('/') {
                        bundle
                    } else {
                        format!("{bundle}/")
                    };
                    found = Some(UiChoice {
                        uri: CStr::from_ptr((host.lilv.lilv_node_as_uri)(uri)).to_owned(),
                        bundle: CString::new(bundle).unwrap_or_default(),
                        binary: PathBuf::from(binary),
                    });
                    break;
                }
            }
            it = (host.lilv.lilv_uis_next)(uis, it);
        }
        (host.lilv.lilv_uis_free)(uis);
        found
    }
}

/// What the UI's callbacks report, read on the next poll.
#[derive(Default)]
struct Bridge {
    writes: RefCell<Vec<(u32, f32)>>,
    size: Cell<Option<(u32, u32)>>,
}

unsafe extern "C" fn write(
    controller: *mut c_void,
    port: u32,
    size: u32,
    protocol: u32,
    buffer: *const c_void,
) {
    if protocol != 0 || size != 4 || buffer.is_null() || controller.is_null() {
        return;
    }
    // SAFETY: the controller is the bridge, alive as long as the UI.
    unsafe {
        let bridge = &*(controller as *const Bridge);
        bridge
            .writes
            .borrow_mut()
            .push((port, *(buffer as *const f32)));
    }
}

unsafe extern "C" fn resize(handle: *mut c_void, width: i32, height: i32) -> i32 {
    if handle.is_null() || width <= 0 || height <= 0 {
        return 1;
    }
    // SAFETY: the handle is the bridge, alive as long as the UI.
    unsafe {
        (*(handle as *const Bridge))
            .size
            .set(Some((width as u32, height as u32)))
    };
    0
}

/// An open LV2 UI.
pub(super) struct Lv2Ui {
    descriptor: *const UiDescriptor,
    handle: *mut c_void,
    idle: *const IdleInterface,
    bridge: Box<Bridge>,
    /// Last value sent to the UI for each port (f32 bits).
    sent: Vec<Option<u32>>,
    window: HostWindow,
    _resize: Box<Resize>,
    _map: Box<UridMap>,
    _unmap: Box<UridUnmap>,
    _options: Box<[OptionEntry]>,
    _rate: Box<f32>,
    _uris: Vec<CString>,
    _features: Vec<Feature>,
    _pointers: Vec<*const Feature>,
    // Dropped last: the UI's code lives in it.
    _library: libloading::Library,
}

impl Lv2Ui {
    /// Loads and embeds the UI; `instance` is the plugin's handle, for UIs that need it.
    pub fn open(
        choice: &UiChoice,
        plugin_uri: &str,
        title: &str,
        instance: *mut c_void,
        ports: usize,
        rate: u32,
    ) -> Result<Self, String> {
        // SAFETY: loading a plugin's UI library runs its initialisers, as any LV2 host does.
        let library = unsafe { libloading::Library::new(&choice.binary) }
            .map_err(|e| format!("cannot load the plugin window: {e}"))?;
        // SAFETY: lv2ui_descriptor has this signature in every LV2 UI library.
        let entry: libloading::Symbol<unsafe extern "C" fn(u32) -> *const UiDescriptor> =
            unsafe { library.get(b"lv2ui_descriptor\0") }
                .map_err(|_| "the plugin window library has no LV2 UI")?;
        let descriptor = (0..)
            // SAFETY: the entry point accepts any index, null past the last descriptor.
            .map(|i| unsafe { entry(i) })
            .take_while(|d| !d.is_null())
            // SAFETY: descriptors returned by the library are valid.
            .find(|&d| unsafe { CStr::from_ptr((*d).uri) } == choice.uri.as_c_str())
            .ok_or("the plugin window is not in its library")?;
        let window = HostWindow::new(title, 640, 480)?;
        let parent = match window.handle() {
            NativeHandle::X11(id) => id as usize as *mut c_void,
            NativeHandle::Win32(hwnd) => hwnd,
        };
        let mut bridge = Box::<Bridge>::default();
        let bridge_ptr = &mut *bridge as *mut Bridge as *mut c_void;
        let mut resize_feature = Box::new(Resize {
            handle: bridge_ptr,
            ui_resize: resize,
        });
        let mut map = Box::new(UridMap {
            handle: std::ptr::null_mut(),
            map: super::urid_map,
        });
        let mut unmap = Box::new(UridUnmap {
            handle: std::ptr::null_mut(),
            unmap: super::urid_unmap,
        });
        let uris: Vec<CString> = [
            "http://lv2plug.in/ns/extensions/ui#parent",
            "http://lv2plug.in/ns/extensions/ui#resize",
            "http://lv2plug.in/ns/extensions/ui#idleInterface",
            "http://lv2plug.in/ns/ext/urid#map",
            "http://lv2plug.in/ns/ext/urid#unmap",
            "http://lv2plug.in/ns/ext/instance-access",
            "http://lv2plug.in/ns/ext/options#options",
        ]
        .iter()
        .map(|u| CString::new(*u).unwrap_or_default())
        .collect();
        let mut features = vec![
            Feature {
                uri: uris[0].as_ptr(),
                data: parent,
            },
            Feature {
                uri: uris[1].as_ptr(),
                data: &mut *resize_feature as *mut Resize as *mut c_void,
            },
            Feature {
                uri: uris[2].as_ptr(),
                data: std::ptr::null_mut(),
            },
            Feature {
                uri: uris[3].as_ptr(),
                data: &mut *map as *mut UridMap as *mut c_void,
            },
            Feature {
                uri: uris[4].as_ptr(),
                data: &mut *unmap as *mut UridUnmap as *mut c_void,
            },
        ];
        let rate = Box::new(rate as f32);
        let mut options: Box<[OptionEntry]> = vec![
            OptionEntry {
                context: 0,
                subject: 0,
                key: super::map_uri("http://lv2plug.in/ns/ext/parameters#sampleRate"),
                size: 4,
                kind: super::map_uri("http://lv2plug.in/ns/ext/atom#Float"),
                value: &*rate as *const f32 as *const c_void,
            },
            OptionEntry {
                context: 0,
                subject: 0,
                key: 0,
                size: 0,
                kind: 0,
                value: std::ptr::null(),
            },
        ]
        .into_boxed_slice();
        features.push(Feature {
            uri: uris[6].as_ptr(),
            data: options.as_mut_ptr() as *mut c_void,
        });
        if !instance.is_null() {
            features.push(Feature {
                uri: uris[5].as_ptr(),
                data: instance,
            });
        }
        let mut pointers: Vec<*const Feature> =
            features.iter().map(|f| f as *const Feature).collect();
        pointers.push(std::ptr::null());
        let plugin_uri = CString::new(plugin_uri).map_err(|e| e.to_string())?;
        let mut widget: *mut c_void = std::ptr::null_mut();
        // SAFETY: every pointer given outlives the UI (kept in the returned value).
        let handle = unsafe {
            let instantiate = (*descriptor)
                .instantiate
                .ok_or("the plugin window cannot be created")?;
            instantiate(
                descriptor,
                plugin_uri.as_ptr(),
                choice.bundle.as_ptr(),
                write,
                bridge_ptr,
                &mut widget,
                pointers.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err("the plugin window failed to open".into());
        }
        // SAFETY: extension_data is a plain query on the descriptor.
        let idle = unsafe {
            (*descriptor)
                .extension_data
                .map_or(std::ptr::null(), |f| f(IDLE_INTERFACE.as_ptr()))
        } as *const IdleInterface;
        let mut ui = Self {
            descriptor,
            handle,
            idle,
            bridge,
            sent: vec![None; ports],
            window,
            _resize: resize_feature,
            _map: map,
            _unmap: unmap,
            _options: options,
            _rate: rate,
            _uris: uris,
            _features: features,
            _pointers: pointers,
            _library: library,
        };
        if let Some((w, h)) = ui.bridge.size.take() {
            ui.window.resize(w, h);
        }
        Ok(ui)
    }

    pub fn raise(&mut self) {
        self.window.raise();
    }

    pub fn take_keys(&mut self) -> Vec<(u32, u16)> {
        self.window.take_keys()
    }

    /// Lets the UI run, sends it the port values that changed and returns the control values
    /// it wrote; `None` once it was closed.
    pub fn poll(&mut self, ports: &[PortInfo], values: &[AtomicU32]) -> Option<Vec<(u32, f32)>> {
        if !self.idle.is_null() {
            // SAFETY: the interface belongs to the live UI.
            if unsafe { ((*self.idle).idle)(self.handle) } != 0 {
                return None;
            }
        }
        if let Some((w, h)) = self.bridge.size.take() {
            self.window.resize(w, h);
        }
        if self.window.poll() {
            return None;
        }
        let writes = std::mem::take(&mut *self.bridge.writes.borrow_mut());
        for &(port, value) in &writes {
            if let Some(sent) = self.sent.get_mut(port as usize) {
                *sent = Some(value.to_bits());
            }
        }
        // SAFETY: port_event belongs to the live UI and gets a float for a control port.
        if let Some(port_event) = unsafe { (*self.descriptor).port_event } {
            for p in ports
                .iter()
                .filter(|p| matches!(p.kind, PortKind::ControlIn | PortKind::ControlOut))
            {
                let i = p.index as usize;
                let Some(bits) = values.get(i).map(|v| v.load(Ordering::Relaxed)) else {
                    continue;
                };
                if self.sent[i] != Some(bits) {
                    self.sent[i] = Some(bits);
                    let value = f32::from_bits(bits);
                    // SAFETY: as above; `value` outlives the call.
                    unsafe {
                        port_event(
                            self.handle,
                            p.index,
                            4,
                            0,
                            &value as *const f32 as *const c_void,
                        )
                    };
                }
            }
        }
        Some(writes)
    }
}

impl Drop for Lv2Ui {
    fn drop(&mut self) {
        // SAFETY: the UI is cleaned up once, before its library and window go.
        unsafe {
            if let Some(cleanup) = (*self.descriptor).cleanup {
                cleanup(self.handle);
            }
        }
    }
}
