//! VST3 host, on the COM bindings of the `vst3` crate.
//!
//! Modules are loaded once and never unloaded. The host provides the objects plugins expect:
//! host application (messages, attribute lists), component handler, memory streams and
//! parameter change queues.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface};

use crate::window::{HostWindow, NativeHandle};
use crate::{Format, MidiEvent, ParamDesc, PluginDescriptor, PluginProcessor, PluginRequest};

const AUDIO_EFFECT_CLASS: &str = "Audio Module Class";
const MAX_QUEUES: usize = 128;
const MAX_POINTS: usize = 16;
const MAX_EVENTS: usize = 512;
/// Controller numbers mapped per MIDI channel: 0-127, aftertouch, pitch bend.
const CONTROLLERS: usize = 130;

fn ok(result: tresult) -> bool {
    result == kResultOk || result == kResultTrue
}

fn c_text(chars: &[char8]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn utf16(chars: &[TChar]) -> String {
    let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end])
}

fn write_utf16(text: &str, out: &mut [TChar]) {
    let mut n = 0;
    for (dst, src) in out.iter_mut().zip(text.encode_utf16()) {
        *dst = src;
        n += 1;
    }
    if let Some(end) = out.get_mut(n.min(out.len().saturating_sub(1))) {
        *end = 0;
    }
}

fn hex(cid: &TUID) -> String {
    cid.iter().map(|&b| format!("{:02X}", b as u8)).collect()
}

fn parse_hex(text: &str) -> Option<TUID> {
    let mut cid = [0 as char8; 16];
    if text.len() != 32 {
        return None;
    }
    for (i, out) in cid.iter_mut().enumerate() {
        *out = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()? as char8;
    }
    Some(cid)
}

fn same_iid(tuid: &TUID, iid: &vst3::com_scrape_types::Guid) -> bool {
    tuid.iter().zip(iid).all(|(&a, &b)| a as u8 == b)
}

/// A loaded module (shared library) and its factory, kept for the life of the process.
struct Module {
    factory: *mut IPluginFactory,
}

// SAFETY: factories are documented thread-safe for class enumeration and creation.
unsafe impl Send for Module {}
// SAFETY: as above.
unsafe impl Sync for Module {}

static MODULES: Mutex<Vec<(PathBuf, &'static Module)>> = Mutex::new(Vec::new());

/// Shared library inside a `.vst3` bundle.
fn library_path(bundle: &Path) -> PathBuf {
    if bundle.is_file() {
        return bundle.to_path_buf();
    }
    let stem = bundle
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let contents = bundle.join("Contents");
    if cfg!(target_os = "windows") {
        contents.join("x86_64-win").join(format!("{stem}.vst3"))
    } else if cfg!(target_os = "macos") {
        contents.join("MacOS").join(stem)
    } else {
        contents
            .join(format!("{}-linux", std::env::consts::ARCH))
            .join(format!("{stem}.so"))
    }
}

fn module(bundle: &Path) -> Result<&'static Module, String> {
    let mut modules = MODULES.lock().map_err(|_| "plugin module list poisoned")?;
    if let Some((_, m)) = modules.iter().find(|(p, _)| p == bundle) {
        return Ok(m);
    }
    #[cfg(target_os = "macos")]
    return bundle_module(&mut modules, bundle);
    #[cfg(not(target_os = "macos"))]
    {
        let path = library_path(bundle);
        if !path.exists() && !cfg!(windows) && bundle.join("Contents").join("x86_64-win").exists() {
            return Err("this bundle only holds a Windows plugin, it cannot run on Linux".into());
        }
        crate::check_binary(&path)?;
        // SAFETY: loading a plugin library runs its initialisers, as in any host.
        let library = unsafe { libloading::Library::new(&path) }
            .map_err(|e| format!("{}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            type Entry = unsafe extern "C" fn(*mut c_void) -> bool;
            // SAFETY: symbol types follow the VST3 module ABI.
            let entry: Option<Entry> = unsafe { library.get::<Entry>(b"ModuleEntry\0") }
                .ok()
                .map(|f| *f);
            if let Some(entry) = entry {
                let raw: libloading::os::unix::Library = library.into();
                let handle = raw.into_raw();
                // SAFETY: the handle comes from dlopen and stays loaded.
                let ok = unsafe { entry(handle) };
                // SAFETY: rebuilding the library from the same handle.
                let library: libloading::Library =
                    unsafe { libloading::os::unix::Library::from_raw(handle) }.into();
                if !ok {
                    return Err("ModuleEntry failed".into());
                }
                return finish(&mut modules, bundle, library);
            }
        }
        #[cfg(windows)]
        {
            type Init = unsafe extern "system" fn() -> bool;
            // SAFETY: symbol types follow the VST3 module ABI.
            if let Ok(init) = unsafe { library.get::<Init>(b"InitDll\0") }
                // SAFETY: InitDll takes no arguments.
                && !unsafe { init() }
            {
                return Err("InitDll failed".into());
            }
        }
        finish(&mut modules, bundle, library)
    }
}

/// A macOS bundle: its executable comes from Info.plist, and `bundleEntry` takes the bundle.
#[cfg(target_os = "macos")]
fn bundle_module(
    modules: &mut Vec<(PathBuf, &'static Module)>,
    bundle: &Path,
) -> Result<&'static Module, String> {
    use objc2_core_foundation::{CFBundle, CFRetained, CFURL};
    let url = CFURL::from_directory_path(bundle).ok_or("this bundle path is not valid")?;
    let cf = CFBundle::new(None, Some(&url)).ok_or("this is not a plugin bundle")?;
    let path = cf
        .executable_url()
        .and_then(|u| u.to_file_path())
        .unwrap_or_else(|| library_path(bundle));
    crate::check_binary(&path)?;
    // SAFETY: loading a plugin library runs its initialisers, as in any host.
    let library = unsafe { libloading::Library::new(&path) }
        .map_err(|e| format!("{}: {e}", path.display()))?;
    type Entry = unsafe extern "C" fn(*mut c_void) -> bool;
    // SAFETY: symbol types follow the VST3 module ABI.
    if let Ok(entry) = unsafe { library.get::<Entry>(b"bundleEntry\0") }
        // SAFETY: the bundle stays alive with the module, for the life of the process.
        && !unsafe { entry(CFRetained::as_ptr(&cf).as_ptr().cast()) }
    {
        return Err("bundleEntry failed".into());
    }
    std::mem::forget(cf);
    finish(modules, bundle, library)
}

fn finish(
    modules: &mut Vec<(PathBuf, &'static Module)>,
    bundle: &Path,
    library: libloading::Library,
) -> Result<&'static Module, String> {
    type GetFactory = unsafe extern "system" fn() -> *mut IPluginFactory;
    // SAFETY: symbol types follow the VST3 module ABI.
    let get =
        unsafe { library.get::<GetFactory>(b"GetPluginFactory\0") }.map_err(|e| e.to_string())?;
    // SAFETY: calling the module's exported factory function.
    let factory = unsafe { get() };
    if factory.is_null() {
        return Err("the module has no plugin factory".into());
    }
    std::mem::forget(library);
    let module: &'static Module = Box::leak(Box::new(Module { factory }));
    modules.push((bundle.to_path_buf(), module));
    Ok(module)
}

fn factory(module: &Module) -> ComRef<'_, IPluginFactory> {
    // SAFETY: the factory pointer is valid for the life of the process.
    unsafe { ComRef::from_raw_unchecked(module.factory) }
}

fn create<I: Interface>(factory: ComRef<'_, IPluginFactory>, cid: &TUID) -> Option<ComPtr<I>> {
    let mut obj: *mut c_void = null_mut();
    // SAFETY: cid and iid are 16-byte identifiers, obj receives an owned reference.
    let result =
        unsafe { factory.createInstance(cid.as_ptr(), I::IID.as_ptr() as FIDString, &mut obj) };
    // SAFETY: on success obj holds a reference to an object implementing I.
    ok(result)
        .then(|| unsafe { ComPtr::from_raw(obj as *mut I) })
        .flatten()
}

/// Lists the audio effects and instruments of a `.vst3` bundle.
pub fn scan(path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    let module = module(path)?;
    let factory = factory(module);
    let factory2 = factory.cast::<IPluginFactory2>();
    let mut vendor = String::new();
    // SAFETY: zeroed C structs are valid, the factory fills them.
    unsafe {
        let mut info: PFactoryInfo = std::mem::zeroed();
        if ok(factory.getFactoryInfo(&mut info)) {
            vendor = c_text(&info.vendor);
        }
    }
    let mut out = Vec::new();
    // SAFETY: indices come from countClasses; structs are plain C data.
    unsafe {
        for i in 0..factory.countClasses() {
            let mut info: PClassInfo2 = std::mem::zeroed();
            let found = match &factory2 {
                Some(f2) => ok(f2.getClassInfo2(i, &mut info)),
                None => {
                    let mut basic: PClassInfo = std::mem::zeroed();
                    let found = ok(factory.getClassInfo(i, &mut basic));
                    info.cid = basic.cid;
                    info.category = basic.category;
                    info.name = basic.name;
                    found
                }
            };
            if !found || c_text(&info.category) != AUDIO_EFFECT_CLASS {
                continue;
            }
            let class_vendor = c_text(&info.vendor);
            let subcategories = c_text(&info.subCategories);
            out.push(PluginDescriptor {
                format: Format::Vst3,
                id: hex(&info.cid),
                name: c_text(&info.name),
                vendor: if class_vendor.is_empty() {
                    vendor.clone()
                } else {
                    class_vendor
                },
                version: c_text(&info.version),
                path: path.to_path_buf(),
                instrument: subcategories.contains("Instrument"),
                tags: tags(&subcategories),
            });
        }
    }
    Ok(out)
}

/// Subcategories such as `Fx|Delay` as tags.
fn tags(subcategories: &str) -> Vec<String> {
    subcategories
        .split('|')
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

// Host-side COM objects.

struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        // SAFETY: the plugin passes a valid String128.
        if let Some(name) = unsafe { name.as_mut() } {
            write_utf16("AudioStrada", name);
        }
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        // SAFETY: pointers come from the plugin and are checked for null.
        let (Some(cid), Some(iid), Some(obj)) =
            (unsafe { cid.as_ref() }, unsafe { iid.as_ref() }, unsafe {
                obj.as_mut()
            })
        else {
            return kInvalidArgument;
        };
        let created = if same_iid(cid, &IMessage::IID) && same_iid(iid, &IMessage::IID) {
            ComWrapper::new(HostMessage::default())
                .to_com_ptr::<IMessage>()
                .map(|p| p.into_raw() as *mut c_void)
        } else if same_iid(cid, &IAttributeList::IID) && same_iid(iid, &IAttributeList::IID) {
            ComWrapper::new(Attributes::default())
                .to_com_ptr::<IAttributeList>()
                .map(|p| p.into_raw() as *mut c_void)
        } else {
            None
        };
        match created {
            Some(p) => {
                *obj = p;
                kResultOk
            }
            None => {
                *obj = null_mut();
                kNoInterface
            }
        }
    }
}

#[derive(Default)]
struct HostMessage {
    id: RefCell<Option<CString>>,
    attributes: RefCell<Option<ComWrapper<Attributes>>>,
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id
            .borrow()
            .as_ref()
            .map_or(std::ptr::null(), |id| id.as_ptr())
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        // SAFETY: the plugin passes a null-terminated string or null.
        *self.id.borrow_mut() = (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned());
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        let mut attributes = self.attributes.borrow_mut();
        let wrapper = attributes.get_or_insert_with(|| ComWrapper::new(Attributes::default()));
        wrapper
            .as_com_ref::<IAttributeList>()
            .map_or(null_mut(), |r| r.as_ptr())
    }
}

#[derive(Clone)]
enum Attribute {
    Int(i64),
    Float(f64),
    Text(Vec<TChar>),
    Binary(Vec<u8>),
}

#[derive(Default)]
struct Attributes {
    values: RefCell<HashMap<CString, Attribute>>,
}

impl Class for Attributes {
    type Interfaces = (IAttributeList,);
}

impl Attributes {
    fn key(id: IAttributeList_::AttrID) -> Option<CString> {
        // SAFETY: attribute ids are null-terminated strings.
        (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned())
    }

    fn set(&self, id: IAttributeList_::AttrID, value: Attribute) -> tresult {
        match Self::key(id) {
            Some(key) => {
                self.values.borrow_mut().insert(key, value);
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    fn get(&self, id: IAttributeList_::AttrID) -> Option<Attribute> {
        self.values.borrow().get(&Self::key(id)?).cloned()
    }
}

impl IAttributeListTrait for Attributes {
    unsafe fn setInt(&self, id: IAttributeList_::AttrID, value: int64) -> tresult {
        self.set(id, Attribute::Int(value))
    }

    unsafe fn getInt(&self, id: IAttributeList_::AttrID, value: *mut int64) -> tresult {
        // SAFETY: the plugin passes a writable value or null.
        match (self.get(id), unsafe { value.as_mut() }) {
            (Some(Attribute::Int(v)), Some(out)) => {
                *out = v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: IAttributeList_::AttrID, value: f64) -> tresult {
        self.set(id, Attribute::Float(value))
    }

    unsafe fn getFloat(&self, id: IAttributeList_::AttrID, value: *mut f64) -> tresult {
        // SAFETY: the plugin passes a writable value or null.
        match (self.get(id), unsafe { value.as_mut() }) {
            (Some(Attribute::Float(v)), Some(out)) => {
                *out = v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: IAttributeList_::AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut text = Vec::new();
        // SAFETY: the plugin passes a null-terminated UTF-16 string.
        unsafe {
            let mut p = string;
            while *p != 0 {
                text.push(*p);
                p = p.add(1);
            }
        }
        self.set(id, Attribute::Text(text))
    }

    unsafe fn getString(
        &self,
        id: IAttributeList_::AttrID,
        string: *mut TChar,
        size_in_bytes: uint32,
    ) -> tresult {
        let Some(Attribute::Text(text)) = self.get(id) else {
            return kResultFalse;
        };
        let capacity = size_in_bytes as usize / 2;
        if string.is_null() || capacity == 0 {
            return kInvalidArgument;
        }
        // SAFETY: the plugin gives a buffer of `size_in_bytes`.
        let out = unsafe { std::slice::from_raw_parts_mut(string, capacity) };
        let n = text.len().min(capacity - 1);
        out[..n].copy_from_slice(&text[..n]);
        out[n] = 0;
        kResultOk
    }

    unsafe fn setBinary(
        &self,
        id: IAttributeList_::AttrID,
        data: *const c_void,
        size_in_bytes: uint32,
    ) -> tresult {
        if data.is_null() && size_in_bytes > 0 {
            return kInvalidArgument;
        }
        let bytes = if size_in_bytes == 0 {
            Vec::new()
        } else {
            // SAFETY: the plugin gives `size_in_bytes` readable bytes.
            unsafe { std::slice::from_raw_parts(data as *const u8, size_in_bytes as usize) }
                .to_vec()
        };
        self.set(id, Attribute::Binary(bytes))
    }

    unsafe fn getBinary(
        &self,
        id: IAttributeList_::AttrID,
        data: *mut *const c_void,
        size_in_bytes: *mut uint32,
    ) -> tresult {
        let Some(key) = Self::key(id) else {
            return kInvalidArgument;
        };
        let values = self.values.borrow();
        // SAFETY: the plugin passes writable out pointers or null.
        match (values.get(&key), unsafe { data.as_mut() }, unsafe {
            size_in_bytes.as_mut()
        }) {
            (Some(Attribute::Binary(bytes)), Some(data), Some(size)) => {
                // The data stays owned by the list, as the API specifies.
                *data = bytes.as_ptr() as *const c_void;
                *size = bytes.len() as uint32;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

/// In-memory IBStream used for plugin state.
#[derive(Default)]
struct MemoryStream {
    data: RefCell<Vec<u8>>,
    position: Cell<usize>,
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl MemoryStream {
    fn with(data: &[u8]) -> ComWrapper<Self> {
        ComWrapper::new(Self {
            data: RefCell::new(data.to_vec()),
            position: Cell::new(0),
        })
    }
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_bytes_read: *mut int32,
    ) -> tresult {
        let data = self.data.borrow();
        let start = self.position.get().min(data.len());
        let n = (num_bytes.max(0) as usize).min(data.len() - start);
        if n > 0 && !buffer.is_null() {
            // SAFETY: the plugin gives a buffer of `num_bytes`.
            unsafe { std::ptr::copy_nonoverlapping(data[start..].as_ptr(), buffer as *mut u8, n) };
        }
        self.position.set(start + n);
        // SAFETY: the plugin passes a writable count or null.
        if let Some(read) = unsafe { num_bytes_read.as_mut() } {
            *read = n as int32;
        }
        kResultOk
    }

    unsafe fn write(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_bytes_written: *mut int32,
    ) -> tresult {
        let n = num_bytes.max(0) as usize;
        if n > 0 && buffer.is_null() {
            return kInvalidArgument;
        }
        let mut data = self.data.borrow_mut();
        let start = self.position.get();
        if data.len() < start + n {
            data.resize(start + n, 0);
        }
        if n > 0 {
            // SAFETY: the plugin gives `num_bytes` readable bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(buffer as *const u8, data[start..].as_mut_ptr(), n)
            };
        }
        self.position.set(start + n);
        // SAFETY: the plugin passes a writable count or null.
        if let Some(written) = unsafe { num_bytes_written.as_mut() } {
            *written = n as int32;
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let len = self.data.borrow().len() as i64;
        #[allow(
            clippy::unnecessary_cast,
            reason = "enum types differ between platforms"
        )]
        let base = match mode as u32 {
            m if m == IBStream_::IStreamSeekMode_::kIBSeekSet as u32 => 0,
            m if m == IBStream_::IStreamSeekMode_::kIBSeekCur as u32 => self.position.get() as i64,
            m if m == IBStream_::IStreamSeekMode_::kIBSeekEnd as u32 => len,
            _ => return kInvalidArgument,
        };
        let target = (base + pos).clamp(0, len);
        self.position.set(target as usize);
        // SAFETY: the plugin passes a writable position or null.
        if let Some(result) = unsafe { result.as_mut() } {
            *result = target;
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        // SAFETY: the plugin passes a writable position or null.
        match unsafe { pos.as_mut() } {
            Some(pos) => {
                *pos = self.position.get() as i64;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
}

/// Receives edits from the plugin's controller (its window).
#[derive(Default)]
struct ComponentHandler {
    edits: Mutex<Vec<(u32, f64)>>,
    restart: Mutex<i32>,
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        if let Ok(mut edits) = self.edits.lock() {
            edits.push((id, value));
        }
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        if let Ok(mut restart) = self.restart.lock() {
            *restart |= flags;
        }
        kResultOk
    }
}

/// A parameter's points within one block; fixed capacity, no allocation.
struct ParamQueue {
    id: Cell<u32>,
    points: RefCell<[(int32, f64); MAX_POINTS]>,
    count: Cell<usize>,
}

impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }

    unsafe fn getPointCount(&self) -> int32 {
        self.count.get() as int32
    }

    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        let i = index as usize;
        if index < 0 || i >= self.count.get() {
            return kInvalidArgument;
        }
        let (o, v) = self.points.borrow()[i];
        // SAFETY: out pointers come from the plugin.
        unsafe {
            if let Some(offset) = offset.as_mut() {
                *offset = o;
            }
            if let Some(value) = value.as_mut() {
                *value = v;
            }
        }
        kResultOk
    }

    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        let n = self.count.get();
        let mut points = self.points.borrow_mut();
        // Points stay sorted by offset; a point at the same offset replaces the previous one.
        let i = match points[..n].iter().position(|p| p.0 >= offset) {
            Some(i) if points[i].0 == offset => {
                points[i].1 = value;
                i
            }
            Some(i) if n < MAX_POINTS => {
                points.copy_within(i..n, i + 1);
                points[i] = (offset, value);
                self.count.set(n + 1);
                i
            }
            None if n < MAX_POINTS => {
                points[n] = (offset, value);
                self.count.set(n + 1);
                n
            }
            _ => return kResultFalse,
        };
        // SAFETY: the plugin passes a writable index or null.
        if let Some(index) = unsafe { index.as_mut() } {
            *index = i as int32;
        }
        kResultOk
    }
}

/// Parameter changes of one block, in either direction.
struct ParamChanges {
    queues: Vec<ComWrapper<ParamQueue>>,
    pointers: Vec<*mut IParamValueQueue>,
    used: Cell<usize>,
}

impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}

impl ParamChanges {
    fn new() -> ComWrapper<Self> {
        let queues: Vec<ComWrapper<ParamQueue>> = (0..MAX_QUEUES)
            .map(|_| {
                ComWrapper::new(ParamQueue {
                    id: Cell::new(0),
                    points: RefCell::new([(0, 0.0); MAX_POINTS]),
                    count: Cell::new(0),
                })
            })
            .collect();
        let pointers = queues
            .iter()
            .map(|q| {
                q.as_com_ref::<IParamValueQueue>()
                    .map_or(null_mut(), |r| r.as_ptr())
            })
            .collect();
        ComWrapper::new(Self {
            queues,
            pointers,
            used: Cell::new(0),
        })
    }

    fn clear(&self) {
        for q in &self.queues[..self.used.get()] {
            q.count.set(0);
        }
        self.used.set(0);
    }

    fn queue(&self, id: u32) -> Option<usize> {
        let used = self.used.get();
        if let Some(i) = self.queues[..used].iter().position(|q| q.id.get() == id) {
            return Some(i);
        }
        if used == MAX_QUEUES {
            return None;
        }
        self.queues[used].id.set(id);
        self.queues[used].count.set(0);
        self.used.set(used + 1);
        Some(used)
    }

    fn push(&self, id: u32, value: f64) {
        if let Some(i) = self.queue(id) {
            // SAFETY: plain call on our own queue object.
            unsafe { self.queues[i].addPoint(0, value, null_mut()) };
        }
    }

    /// Last value of each changed parameter.
    fn last_values(&self, mut each: impl FnMut(u32, f64)) {
        for q in &self.queues[..self.used.get()] {
            let n = q.count.get();
            if n > 0 {
                each(q.id.get(), q.points.borrow()[n - 1].1);
            }
        }
    }
}

impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.used.get() as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if index >= 0 && (index as usize) < self.used.get() {
            self.pointers[index as usize]
        } else {
            null_mut()
        }
    }

    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut int32,
    ) -> *mut IParamValueQueue {
        // SAFETY: the plugin passes a valid parameter id pointer.
        let Some(&id) = (unsafe { id.as_ref() }) else {
            return null_mut();
        };
        match self.queue(id) {
            Some(i) => {
                // SAFETY: the plugin passes a writable index or null.
                if let Some(index) = unsafe { index.as_mut() } {
                    *index = i as int32;
                }
                self.pointers[i]
            }
            None => null_mut(),
        }
    }
}

/// Note events of one block; fixed capacity, no allocation.
struct EventList {
    events: RefCell<Vec<Event>>,
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl EventList {
    fn new() -> ComWrapper<Self> {
        ComWrapper::new(Self {
            events: RefCell::new(Vec::with_capacity(MAX_EVENTS)),
        })
    }

    fn push(&self, event: Event) {
        let mut events = self.events.borrow_mut();
        if events.len() < MAX_EVENTS {
            events.push(event);
        }
    }
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.events.borrow().len() as int32
    }

    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        let events = self.events.borrow();
        match (
            usize::try_from(index).ok().and_then(|i| events.get(i)),
            // SAFETY: the plugin passes a writable event or null.
            unsafe { e.as_mut() },
        ) {
            (Some(event), Some(out)) => {
                *out = *event;
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        // SAFETY: the plugin passes a readable event or null.
        match unsafe { e.as_ref() } {
            Some(event) => {
                self.push(*event);
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
}

fn note_event(offset: u32, kind: uint16, payload: Event__type0) -> Event {
    #[allow(
        clippy::unnecessary_cast,
        reason = "enum types differ between platforms"
    )]
    Event {
        busIndex: 0,
        sampleOffset: offset as int32,
        ppqPosition: 0.0,
        flags: Event_::EventFlags_::kIsLive as uint16,
        r#type: kind,
        __field0: payload,
    }
}

/// Host frame of an editor: resize requests, and on Linux the run loop serving the plugin's
/// file descriptors and timers.
#[derive(Default)]
struct PlugFrame {
    resize: Cell<Option<(u32, u32)>>,
    handlers: RefCell<Vec<(ComPtr<Linux::IEventHandler>, Linux::FileDescriptor)>>,
    timers: RefCell<Vec<(ComPtr<Linux::ITimerHandler>, Duration, Instant)>>,
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame, Linux::IRunLoop);
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, _view: *mut IPlugView, size: *mut ViewRect) -> tresult {
        // SAFETY: the plugin passes a valid rectangle.
        match unsafe { size.as_ref() } {
            Some(r) => {
                let (w, h) = (
                    (r.right - r.left).max(1) as u32,
                    (r.bottom - r.top).max(1) as u32,
                );
                self.resize.set(Some((w, h)));
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
}

impl Linux::IRunLoopTrait for PlugFrame {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut Linux::IEventHandler,
        fd: Linux::FileDescriptor,
    ) -> tresult {
        // SAFETY: the plugin passes a live handler; we keep our own reference.
        match unsafe { ComRef::from_raw(handler) } {
            Some(h) => {
                self.handlers.borrow_mut().push((h.to_com_ptr(), fd));
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn unregisterEventHandler(&self, handler: *mut Linux::IEventHandler) -> tresult {
        self.handlers
            .borrow_mut()
            .retain(|(h, _)| h.as_ptr() != handler);
        kResultOk
    }

    unsafe fn registerTimer(
        &self,
        handler: *mut Linux::ITimerHandler,
        milliseconds: Linux::TimerInterval,
    ) -> tresult {
        // SAFETY: as for event handlers.
        match unsafe { ComRef::from_raw(handler) } {
            Some(h) => {
                let period = Duration::from_millis(milliseconds.max(10));
                self.timers
                    .borrow_mut()
                    .push((h.to_com_ptr(), period, Instant::now() + period));
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn unregisterTimer(&self, handler: *mut Linux::ITimerHandler) -> tresult {
        self.timers
            .borrow_mut()
            .retain(|(h, _, _)| h.as_ptr() != handler);
        kResultOk
    }
}

impl PlugFrame {
    /// Serves the plugin's timers and ready file descriptors, without blocking.
    fn run(&self) {
        let now = Instant::now();
        let due: Vec<ComPtr<Linux::ITimerHandler>> = self
            .timers
            .borrow_mut()
            .iter_mut()
            .filter(|t| t.2 <= now)
            .map(|t| {
                t.2 = now + t.1;
                t.0.clone()
            })
            .collect();
        for timer in due {
            // SAFETY: we hold a reference to the handler.
            unsafe { Linux::ITimerHandlerTrait::onTimer(&timer) };
        }
        #[cfg(unix)]
        {
            let handlers = self.handlers.borrow().clone();
            if handlers.is_empty() {
                return;
            }
            let mut polled: Vec<libc::pollfd> = handlers
                .iter()
                .map(|(_, fd)| libc::pollfd {
                    fd: *fd,
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            // SAFETY: the array is valid for its length; a zero timeout never blocks.
            if unsafe { libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, 0) } > 0 {
                for ((handler, fd), p) in handlers.iter().zip(&polled) {
                    if p.revents != 0 {
                        // SAFETY: we hold a reference to the handler.
                        unsafe { Linux::IEventHandlerTrait::onFDIsSet(handler, *fd) };
                    }
                }
            }
        }
    }
}

struct Editor {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<PlugFrame>,
    window: HostWindow,
}

#[cfg(windows)]
const PLATFORM: FIDString = kPlatformTypeHWND;
#[cfg(target_os = "macos")]
const PLATFORM: FIDString = kPlatformTypeNSView;
#[cfg(all(not(windows), not(target_os = "macos")))]
const PLATFORM: FIDString = kPlatformTypeX11EmbedWindowID;

/// Main-thread side of a VST3 plugin.
pub struct Vst3Instance {
    descriptor: PluginDescriptor,
    component: ComPtr<IComponent>,
    controller: Option<ComPtr<IEditController>>,
    /// The controller is a separate object (not the component itself).
    separate: bool,
    _host: ComWrapper<HostApplication>,
    handler: ComWrapper<ComponentHandler>,
    active: bool,
    processor_alive: Arc<AtomicBool>,
    editor: Option<Editor>,
    has_editor: Option<bool>,
}

impl std::fmt::Debug for Vst3Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst3Instance")
            .field("plugin", &self.descriptor.name)
            .finish_non_exhaustive()
    }
}

fn connection(
    component: &ComPtr<IComponent>,
    controller: &ComPtr<IEditController>,
) -> Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)> {
    Some((
        component.cast::<IConnectionPoint>()?,
        controller.cast::<IConnectionPoint>()?,
    ))
}

impl Vst3Instance {
    pub fn new(descriptor: &PluginDescriptor) -> Result<Self, String> {
        let module = module(&descriptor.path)?;
        let factory = factory(module);
        let cid = parse_hex(&descriptor.id).ok_or("invalid VST3 class id")?;
        let component: ComPtr<IComponent> =
            create(factory, &cid).ok_or("cannot create the plugin")?;
        let host = ComWrapper::new(HostApplication);
        let context = host
            .as_com_ref::<IHostApplication>()
            .map_or(null_mut(), |r| r.as_ptr() as *mut FUnknown);
        // SAFETY: the context outlives the component (kept in the instance).
        if !ok(unsafe { component.initialize(context) }) {
            return Err("the plugin failed to initialize".into());
        }
        let (controller, separate) = match component.cast::<IEditController>() {
            Some(controller) => (Some(controller), false),
            None => {
                let mut controller_cid = [0 as char8; 16];
                // SAFETY: the component writes a 16-byte class id.
                let controller =
                    if ok(unsafe { component.getControllerClassId(&mut controller_cid) }) {
                        create::<IEditController>(factory, &controller_cid)
                    } else {
                        None
                    };
                // SAFETY: as for the component.
                let controller = controller.filter(|c| ok(unsafe { c.initialize(context) }));
                (controller, true)
            }
        };
        let handler = ComWrapper::new(ComponentHandler::default());
        if let Some(controller) = &controller {
            if separate && let Some((a, b)) = connection(&component, controller) {
                // SAFETY: both connection points stay alive until disconnected in Drop.
                unsafe {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
            }
            if let Some(h) = handler.as_com_ref::<IComponentHandler>() {
                // SAFETY: the handler outlives the controller.
                unsafe { controller.setComponentHandler(h.as_ptr()) };
            }
        }
        let mut instance = Self {
            descriptor: descriptor.clone(),
            component,
            controller,
            separate,
            _host: host,
            handler,
            active: false,
            processor_alive: Arc::new(AtomicBool::new(false)),
            editor: None,
            has_editor: None,
        };
        instance.sync_controller();
        Ok(instance)
    }

    /// Gives the component state to a separate controller.
    fn sync_controller(&mut self) {
        let (Some(controller), true) = (&self.controller, self.separate) else {
            return;
        };
        let stream = MemoryStream::with(&[]);
        let Some(s) = stream.as_com_ref::<IBStream>() else {
            return;
        };
        // SAFETY: the stream lives for the duration of the calls.
        unsafe {
            if ok(self.component.getState(s.as_ptr())) {
                stream.position.set(0);
                controller.setComponentState(s.as_ptr());
            }
        }
    }

    /// The plugin's editor view, if it has one for this platform.
    fn create_view(&self) -> Option<ComPtr<IPlugView>> {
        let controller = self.controller.as_ref()?;
        // SAFETY: createView returns an owned reference or null.
        let view = unsafe { ComPtr::from_raw(controller.createView(ViewType::kEditor)) }?;
        // SAFETY: plain call on the live view.
        ok(unsafe { view.isPlatformTypeSupported(PLATFORM) }).then_some(view)
    }

    /// Channel count of a main bus arrangement.
    fn channels(arrangement: SpeakerArrangement) -> usize {
        arrangement.count_ones() as usize
    }
}

/// Component and controller states in one blob: `SV3\0`, the component's length, both states.
fn pack_state(component: &[u8], controller: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + component.len() + controller.len());
    out.extend_from_slice(b"SV3\0");
    out.extend_from_slice(&(component.len() as u32).to_le_bytes());
    out.extend_from_slice(component);
    out.extend_from_slice(controller);
    out
}

/// The component and controller states of [`pack_state`].
fn unpack_state(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = data.strip_prefix(b"SV3\0")?;
    let (len, rest) = rest.split_first_chunk::<4>()?;
    let len = u32::from_le_bytes(*len) as usize;
    (len <= rest.len()).then(|| rest.split_at(len))
}

impl crate::PluginInstance for Vst3Instance {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        max_frames: usize,
    ) -> Result<Box<dyn PluginProcessor>, String> {
        let processor = self
            .component
            .cast::<IAudioProcessor>()
            .ok_or("the plugin cannot process audio")?;
        let audio = MediaTypes_::kAudio as MediaType;
        let (input, output) = (
            BusDirections_::kInput as BusDirection,
            BusDirections_::kOutput as BusDirection,
        );
        // SAFETY: plain calls on a live component; arrays outlive the calls.
        let (inputs, outputs) = unsafe {
            let n_in = self.component.getBusCount(audio, input).max(0) as usize;
            let n_out = self.component.getBusCount(audio, output).max(0) as usize;
            let current = |dir: BusDirection, i: usize| {
                let mut arrangement: SpeakerArrangement = 0;
                processor.getBusArrangement(dir, i as int32, &mut arrangement);
                arrangement
            };
            let mut ins: Vec<SpeakerArrangement> = (0..n_in).map(|i| current(input, i)).collect();
            let mut outs: Vec<SpeakerArrangement> =
                (0..n_out).map(|i| current(output, i)).collect();
            if let Some(main) = ins.first_mut() {
                *main = SpeakerArr::kStereo;
            }
            if let Some(main) = outs.first_mut() {
                *main = SpeakerArr::kStereo;
            }
            processor.setBusArrangements(
                ins.as_mut_ptr(),
                n_in as int32,
                outs.as_mut_ptr(),
                n_out as int32,
            );
            for (dir, count) in [(input, n_in), (output, n_out)] {
                for i in 0..count {
                    self.component
                        .activateBus(audio, dir, i as int32, (i == 0) as TBool);
                }
            }
            let event = MediaTypes_::kEvent as MediaType;
            if self.component.getBusCount(event, input) > 0 {
                self.component.activateBus(event, input, 0, 1);
            }
            let main = |dir: BusDirection, count: usize| {
                if count == 0 {
                    0
                } else {
                    Self::channels(current(dir, 0))
                }
            };
            (main(input, n_in), main(output, n_out))
        };
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: max_frames.max(1) as int32,
            sampleRate: f64::from(sample_rate),
        };
        // SAFETY: the setup struct lives for the call.
        unsafe {
            if !ok(processor.setupProcessing(&mut setup)) {
                return Err("the plugin refused the audio format".into());
            }
            if !ok(self.component.setActive(1)) {
                return Err("the plugin failed to activate".into());
            }
        }
        self.active = true;
        self.processor_alive.store(true, Ordering::Relaxed);
        let max = max_frames.max(1);
        let mut buffers_in: Vec<Vec<f32>> = vec![vec![0.0; max]; inputs];
        let mut buffers_out: Vec<Vec<f32>> = vec![vec![0.0; max]; outputs];
        let in_ptrs = buffers_in.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let out_ptrs = buffers_out.iter_mut().map(|b| b.as_mut_ptr()).collect();
        // SAFETY: an all-zero ProcessContext is a valid "nothing known" context.
        let mut context: ProcessContext = unsafe { std::mem::zeroed() };
        context.sampleRate = f64::from(sample_rate);
        context.tempo = 120.0;
        context.timeSigNumerator = 4;
        context.timeSigDenominator = 4;
        #[allow(
            clippy::unnecessary_cast,
            reason = "enum types differ between platforms"
        )]
        {
            context.state = ProcessContext_::StatesAndFlags_::kTempoValid as u32;
        }
        let mut controllers = Vec::new();
        if let Some(mapping) = self
            .controller
            .as_ref()
            .and_then(|c| c.cast::<IMidiMapping>())
        {
            controllers = vec![None; 16 * CONTROLLERS];
            for (i, slot) in controllers.iter_mut().enumerate() {
                let mut id: ParamID = 0;
                // SAFETY: plain query on the live controller.
                let found = unsafe {
                    mapping.getMidiControllerAssignment(
                        0,
                        (i / CONTROLLERS) as int16,
                        (i % CONTROLLERS) as CtrlNumber,
                        &mut id,
                    )
                };
                if ok(found) {
                    *slot = Some(id);
                }
            }
        }
        Ok(Box::new(Vst3Processor {
            processor,
            buffers_in,
            buffers_out,
            in_ptrs,
            out_ptrs,
            changes_in: ParamChanges::new(),
            changes_out: ParamChanges::new(),
            events: EventList::new(),
            controllers,
            context,
            processing: false,
            alive: Arc::clone(&self.processor_alive),
        }))
    }

    fn try_deactivate(&mut self) -> bool {
        if self.processor_alive.load(Ordering::Relaxed) {
            return false;
        }
        if self.active {
            // SAFETY: plain call on a live component.
            unsafe { self.component.setActive(0) };
            self.active = false;
        }
        true
    }

    fn params(&mut self) -> Vec<ParamDesc> {
        let Some(controller) = &self.controller else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // SAFETY: indices come from getParameterCount; ParameterInfo is plain C data.
        unsafe {
            for i in 0..controller.getParameterCount() {
                let mut info: ParameterInfo = std::mem::zeroed();
                if !ok(controller.getParameterInfo(i, &mut info)) {
                    continue;
                }
                let flags = info.flags;
                let units = utf16(&info.units);
                out.push(ParamDesc {
                    id: info.id,
                    name: utf16(&info.title),
                    module: units,
                    min: 0.0,
                    max: 1.0,
                    default: info.defaultNormalizedValue,
                    stepped: info.stepCount > 0,
                    hidden: flags & ParameterInfo_::ParameterFlags_::kIsHidden != 0,
                    read_only: flags & ParameterInfo_::ParameterFlags_::kIsReadOnly != 0,
                });
            }
        }
        out
    }

    fn param_value(&mut self, id: u32) -> Option<f64> {
        // SAFETY: plain call on a live controller.
        self.controller
            .as_ref()
            .map(|c| unsafe { c.getParamNormalized(id) })
    }

    fn value_text(&mut self, id: u32, value: f64) -> Option<String> {
        let controller = self.controller.as_ref()?;
        let mut text: String128 = [0; 128];
        // SAFETY: the controller writes into a String128.
        ok(unsafe { controller.getParamStringByValue(id, value, &mut text) }).then(|| utf16(&text))
    }

    fn set_param(&mut self, id: u32, value: f64) {
        if let Some(controller) = &self.controller {
            // SAFETY: plain call on a live controller.
            unsafe { controller.setParamNormalized(id, value) };
        }
    }

    fn latency(&mut self) -> u32 {
        self.component
            .cast::<IAudioProcessor>()
            // SAFETY: plain call on a live processor.
            .map_or(0, |p| unsafe { p.getLatencySamples() })
    }

    fn save_state(&mut self) -> Option<Vec<u8>> {
        let component = MemoryStream::with(&[]);
        let controller = MemoryStream::with(&[]);
        // SAFETY: streams live for the duration of the calls.
        unsafe {
            let s = component.as_com_ref::<IBStream>()?;
            if !ok(self.component.getState(s.as_ptr())) {
                return None;
            }
            if let (Some(c), true) = (&self.controller, self.separate) {
                let s = controller.as_com_ref::<IBStream>()?;
                c.getState(s.as_ptr());
            }
        }
        Some(pack_state(
            &component.data.borrow(),
            &controller.data.borrow(),
        ))
    }

    fn load_state(&mut self, data: &[u8]) -> bool {
        let Some((component_state, controller_state)) = unpack_state(data) else {
            return false;
        };
        let stream = MemoryStream::with(component_state);
        // SAFETY: streams live for the duration of the calls.
        unsafe {
            let Some(s) = stream.as_com_ref::<IBStream>() else {
                return false;
            };
            if !ok(self.component.setState(s.as_ptr())) {
                return false;
            }
            if let (Some(c), true) = (&self.controller, self.separate) {
                stream.position.set(0);
                c.setComponentState(s.as_ptr());
                let extra = MemoryStream::with(controller_state);
                if let Some(e) = extra.as_com_ref::<IBStream>() {
                    c.setState(e.as_ptr());
                }
            }
        }
        true
    }

    fn poll(&mut self) -> Vec<PluginRequest> {
        let mut requests = Vec::new();
        if let Ok(mut edits) = self.handler.edits.lock() {
            requests.extend(
                edits
                    .drain(..)
                    .map(|(id, v)| PluginRequest::ParamChanged(id, v)),
            );
        }
        let flags = self
            .handler
            .restart
            .lock()
            .map(|mut r| std::mem::take(&mut *r))
            .unwrap_or(0);
        if flags & RestartFlags_::kLatencyChanged != 0 {
            requests.push(PluginRequest::LatencyChanged);
        }
        if flags & RestartFlags_::kReloadComponent != 0 {
            requests.push(PluginRequest::Restart);
        }
        if flags & RestartFlags_::kParamValuesChanged != 0 {
            requests.push(PluginRequest::RescanParams);
        }
        let mut closed = false;
        if let Some(editor) = &mut self.editor {
            editor.frame.run();
            if let Some((w, h)) = editor.frame.resize.take() {
                editor.window.resize(w, h);
                let mut rect = ViewRect {
                    left: 0,
                    top: 0,
                    right: w as int32,
                    bottom: h as int32,
                };
                // SAFETY: plain call on the attached view.
                unsafe { editor.view.onSize(&mut rect) };
            }
            closed = editor.window.poll();
            requests.extend(
                editor
                    .window
                    .take_keys()
                    .into_iter()
                    .map(|(sym, mods)| PluginRequest::Key(sym, mods)),
            );
        }
        if closed {
            self.close_gui();
            requests.push(PluginRequest::GuiClosed);
        }
        requests
    }

    fn has_gui(&mut self) -> bool {
        if self.has_editor.is_none() {
            self.has_editor = Some(self.editor.is_some() || self.create_view().is_some());
        }
        self.has_editor == Some(true)
    }

    fn open_gui(&mut self, title: &str) -> Result<(), String> {
        if self.editor.is_some() {
            return Ok(());
        }
        let view = self
            .create_view()
            .ok_or("this plugin has no editor for this system")?;
        // SAFETY: ViewRect is plain data filled by the view.
        let mut rect: ViewRect = unsafe { std::mem::zeroed() };
        // SAFETY: the view fills the rectangle.
        let (width, height) = if ok(unsafe { view.getSize(&mut rect) }) {
            (
                (rect.right - rect.left).max(100) as u32,
                (rect.bottom - rect.top).max(100) as u32,
            )
        } else {
            (640, 480)
        };
        let window = HostWindow::new(title, width, height)?;
        let frame = ComWrapper::new(PlugFrame::default());
        let parent = match window.handle() {
            NativeHandle::X11(id) => id as usize as *mut c_void,
            NativeHandle::Win32(hwnd) | NativeHandle::Cocoa(hwnd) => hwnd,
        };
        // SAFETY: the frame and window outlive the attachment (kept in the editor).
        unsafe {
            if let Some(f) = frame.as_com_ref::<IPlugFrame>() {
                view.setFrame(f.as_ptr());
            }
            if !ok(view.attached(parent, PLATFORM)) {
                view.setFrame(null_mut());
                return Err("the plugin could not open its editor".into());
            }
        }
        self.editor = Some(Editor {
            view,
            frame,
            window,
        });
        Ok(())
    }

    fn close_gui(&mut self) {
        if let Some(editor) = self.editor.take() {
            // SAFETY: detaching in the documented order, before the window goes away.
            unsafe {
                editor.view.removed();
                editor.view.setFrame(null_mut());
            }
        }
    }

    fn gui_open(&self) -> bool {
        self.editor.is_some()
    }

    fn raise_gui(&mut self) {
        if let Some(editor) = &mut self.editor {
            editor.window.raise();
        }
    }
}

impl Drop for Vst3Instance {
    fn drop(&mut self) {
        crate::PluginInstance::close_gui(self);
        if self.processor_alive.load(Ordering::Relaxed) {
            // The audio side still uses the plugin: leak a reference rather than free under it.
            std::mem::forget(self.component.clone());
            std::mem::forget(self.controller.clone());
            return;
        }
        // SAFETY: plain calls on live objects, in the documented teardown order.
        unsafe {
            if self.active {
                self.component.setActive(0);
            }
            if let Some(controller) = self.controller.take() {
                if self.separate {
                    if let Some((a, b)) = connection(&self.component, &controller) {
                        a.disconnect(b.as_ptr());
                        b.disconnect(a.as_ptr());
                    }
                    controller.terminate();
                }
                controller.setComponentHandler(null_mut());
            }
            self.component.terminate();
        }
    }
}

/// Audio side of a VST3 plugin.
pub struct Vst3Processor {
    processor: ComPtr<IAudioProcessor>,
    buffers_in: Vec<Vec<f32>>,
    buffers_out: Vec<Vec<f32>>,
    in_ptrs: Vec<*mut f32>,
    out_ptrs: Vec<*mut f32>,
    changes_in: ComWrapper<ParamChanges>,
    changes_out: ComWrapper<ParamChanges>,
    events: ComWrapper<EventList>,
    /// Parameter assigned to each (channel, controller), from the plugin's MIDI mapping.
    controllers: Vec<Option<ParamID>>,
    context: ProcessContext,
    processing: bool,
    alive: Arc<AtomicBool>,
}

// SAFETY: the processor is used by one audio thread at a time, as VST3 requires; the COM
// objects it owns are not shared with other threads while it runs.
unsafe impl Send for Vst3Processor {}

impl std::fmt::Debug for Vst3Processor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst3Processor").finish_non_exhaustive()
    }
}

impl Vst3Processor {
    fn midi_event(&self, e: &MidiEvent, time: u32) {
        let channel = e.channel();
        #[allow(
            clippy::unnecessary_cast,
            reason = "enum types differ between platforms"
        )]
        if let Some((key, velocity)) = e.as_note_on() {
            let on = NoteOnEvent {
                channel: i16::from(channel),
                pitch: i16::from(key),
                tuning: 0.0,
                velocity: f32::from(velocity) / 127.0,
                length: 0,
                noteId: -1,
            };
            let kind = Event_::EventTypes_::kNoteOnEvent as uint16;
            self.events
                .push(note_event(time, kind, Event__type0 { noteOn: on }));
        } else if let Some(key) = e.as_note_off() {
            let off = NoteOffEvent {
                channel: i16::from(channel),
                pitch: i16::from(key),
                velocity: 0.0,
                noteId: -1,
                tuning: 0.0,
            };
            let kind = Event_::EventTypes_::kNoteOffEvent as uint16;
            self.events
                .push(note_event(time, kind, Event__type0 { noteOff: off }));
        } else {
            let (controller, value) = match e.data[0] & 0xF0 {
                0xB0 => (usize::from(e.data[1]), f64::from(e.data[2]) / 127.0),
                0xD0 => (128, f64::from(e.data[1]) / 127.0),
                0xE0 => {
                    let bend = u16::from(e.data[2]) << 7 | u16::from(e.data[1]);
                    (129, f64::from(bend) / 16383.0)
                }
                _ => return,
            };
            let slot = usize::from(channel) * CONTROLLERS + controller;
            if let Some(Some(id)) = self.controllers.get(slot)
                && let Some(i) = self.changes_in.queue(*id)
            {
                // SAFETY: plain call on our own queue object.
                unsafe { self.changes_in.queues[i].addPoint(time as int32, value, null_mut()) };
            }
        }
    }
}

impl PluginProcessor for Vst3Processor {
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        params_out: &mut dyn FnMut(u32, f64),
    ) {
        let max = self.buffers_out.first().map_or(0, Vec::len);
        let n = left.len().min(right.len()).min(max);
        if n == 0 {
            return;
        }
        if !self.processing {
            // SAFETY: plain call on the live processor, from the audio thread.
            unsafe { self.processor.setProcessing(1) };
            self.processing = true;
        }
        self.changes_in.clear();
        self.changes_out.clear();
        for &(id, value) in params_in {
            self.changes_in.push(id, value);
        }
        self.events.events.borrow_mut().clear();
        for e in midi {
            self.midi_event(e, crate::block_time(e, n));
        }
        crate::fill_inputs(&mut self.buffers_in, left, right, n);
        let mut input = AudioBusBuffers {
            numChannels: self.in_ptrs.len() as int32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: self.in_ptrs.as_mut_ptr(),
            },
        };
        let mut output = AudioBusBuffers {
            numChannels: self.out_ptrs.len() as int32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: self.out_ptrs.as_mut_ptr(),
            },
        };
        let changes_in = self
            .changes_in
            .as_com_ref::<IParameterChanges>()
            .map_or(null_mut(), |r| r.as_ptr());
        let changes_out = self
            .changes_out
            .as_com_ref::<IParameterChanges>()
            .map_or(null_mut(), |r| r.as_ptr());
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: n as int32,
            numInputs: i32::from(!self.in_ptrs.is_empty()),
            numOutputs: i32::from(!self.out_ptrs.is_empty()),
            inputs: &mut input,
            outputs: &mut output,
            inputParameterChanges: changes_in,
            outputParameterChanges: changes_out,
            inputEvents: self
                .events
                .as_com_ref::<IEventList>()
                .map_or(null_mut(), |r| r.as_ptr()),
            outputEvents: null_mut(),
            processContext: &mut self.context,
        };
        // SAFETY: every buffer and object referenced by `data` lives for the call.
        let result = unsafe { self.processor.process(&mut data) };
        self.context.projectTimeSamples += n as i64;
        self.context.continousTimeSamples += n as i64;
        if ok(result) {
            crate::take_outputs(&self.buffers_out, left, right, n);
        }
        self.changes_out.last_values(&mut *params_out);
    }

    fn reset(&mut self) {}
}

impl Drop for Vst3Processor {
    fn drop(&mut self) {
        if self.processing {
            // SAFETY: plain call on the live processor.
            unsafe { self.processor.setProcessing(0) };
        }
        self.alive.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginInstance as _;

    #[test]
    fn identifiers_and_streams() {
        let cid: TUID = std::array::from_fn(|i| (i * 17) as u8 as char8);
        assert_eq!(parse_hex(&hex(&cid)), Some(cid));
        assert!(parse_hex("zz").is_none());
        let stream = MemoryStream::with(b"abc");
        let s = stream.as_com_ref::<IBStream>().unwrap();
        let mut buf = [0u8; 8];
        let mut read = 0;
        // SAFETY: test buffers outlive the calls.
        unsafe {
            s.read(buf.as_mut_ptr() as *mut c_void, 8, &mut read);
            assert_eq!((read, &buf[..3]), (3, &b"abc"[..]));
            let mut pos = 0;
            s.seek(
                1,
                IBStream_::IStreamSeekMode_::kIBSeekSet as int32,
                &mut pos,
            );
            let mut written = 0;
            s.write(b"XYZ".as_ptr() as *mut c_void, 3, &mut written);
        }
        assert_eq!(&*stream.data.borrow(), b"aXYZ");
        let changes = ParamChanges::new();
        changes.push(7, 0.25);
        changes.push(7, 0.5);
        changes.push(9, 1.0);
        let mut seen = Vec::new();
        changes.last_values(|id, v| seen.push((id, v)));
        assert_eq!(seen, vec![(7, 0.5), (9, 1.0)]);
    }

    #[test]
    fn states_and_texts() {
        let packed = pack_state(b"comp", b"ctl");
        assert_eq!(unpack_state(&packed), Some((&b"comp"[..], &b"ctl"[..])));
        assert_eq!(
            unpack_state(&pack_state(b"", b"")),
            Some((&[][..], &[][..]))
        );
        assert_eq!(unpack_state(&packed[..9]), None);
        assert_eq!(unpack_state(b"LV2P\0\0\0\0"), None);
        assert_eq!(tags("Fx|Delay||"), vec!["Fx", "Delay"]);
        let mut out = [7 as TChar; 4];
        write_utf16("AudioStrada", &mut out);
        assert_eq!(utf16(&out), "Aud");
        assert_eq!(c_text(&[b'a' as char8, 0, b'b' as char8]), "a");
    }

    #[test]
    fn host_objects_follow_the_api() {
        let attributes = ComWrapper::new(Attributes::default());
        let list = attributes.as_com_ref::<IAttributeList>().unwrap();
        let text: Vec<TChar> = "hé\0".encode_utf16().collect();
        let mut out = [0 as TChar; 2];
        let (mut int, mut float) = (0i64, 0f64);
        let (mut data, mut size) = (std::ptr::null(), 0u32);
        // SAFETY: ids are null-terminated and out pointers live for the calls.
        unsafe {
            assert_eq!(list.setInt(c"i".as_ptr(), 5), kResultOk);
            assert_eq!(list.getInt(c"i".as_ptr(), &mut int), kResultOk);
            assert_eq!(list.getFloat(c"i".as_ptr(), &mut float), kResultFalse);
            list.setFloat(c"f".as_ptr(), 0.5);
            list.getFloat(c"f".as_ptr(), &mut float);
            list.setString(c"s".as_ptr(), text.as_ptr());
            list.getString(c"s".as_ptr(), out.as_mut_ptr(), 4);
            list.setBinary(c"b".as_ptr(), b"xy".as_ptr().cast(), 2);
            list.getBinary(c"b".as_ptr(), &mut data, &mut size);
            assert_eq!(list.setInt(std::ptr::null(), 1), kInvalidArgument);
            assert_eq!(std::slice::from_raw_parts(data.cast::<u8>(), 2), b"xy");
        }
        assert_eq!((int, float, size), (5, 0.5, 2));
        assert_eq!(utf16(&out), "h");

        let queue = ParamChanges::new();
        let q = &queue.queues[queue.queue(3).unwrap()];
        let mut index = -1;
        // SAFETY: plain calls on our own objects with live out pointers.
        unsafe {
            q.addPoint(10, 0.1, &mut index);
            q.addPoint(2, 0.2, &mut index);
            assert_eq!(index, 0);
            q.addPoint(10, 0.3, &mut index);
            assert_eq!((index, q.getPointCount()), (1, 2));
            let (mut offset, mut value) = (0, 0.0);
            q.getPoint(1, &mut offset, &mut value);
            assert_eq!((offset, value), (10, 0.3));
            assert_eq!(q.getPoint(2, &mut offset, &mut value), kInvalidArgument);
            for i in 0..MAX_POINTS as i32 {
                q.addPoint(100 + i, 0.0, null_mut());
            }
            assert_eq!(q.getPointCount(), MAX_POINTS as int32);
            let changes = queue.as_com_ref::<IParameterChanges>().unwrap();
            assert_eq!(changes.getParameterCount(), 1);
            assert!(changes.getParameterData(1).is_null());
            let id: ParamID = 9;
            assert!(!changes.addParameterData(&id, &mut index).is_null());
            assert_eq!(index, 1);
        }
        for id in 0..MAX_QUEUES as u32 {
            queue.push(id + 100, 1.0);
        }
        assert_eq!(queue.queue(1000), None);
        queue.clear();
        assert_eq!(queue.used.get(), 0);

        let events = EventList::new();
        let list = events.as_com_ref::<IEventList>().unwrap();
        // SAFETY: an all-zero note is valid plain data.
        let event = note_event(
            4,
            0,
            Event__type0 {
                noteOn: unsafe { std::mem::zeroed() },
            },
        );
        // SAFETY: events are plain data living for the calls.
        unsafe {
            for _ in 0..=MAX_EVENTS {
                list.addEvent(&event as *const Event as *mut Event);
            }
            assert_eq!(list.getEventCount(), MAX_EVENTS as int32);
            let mut out: Event = std::mem::zeroed();
            assert_eq!(list.getEvent(0, &mut out), kResultOk);
            assert_eq!(out.sampleOffset, 4);
            assert_eq!(list.getEvent(-1, &mut out), kInvalidArgument);
        }

        let stream = MemoryStream::with(b"abcd");
        let s = stream.as_com_ref::<IBStream>().unwrap();
        let mut pos = 0;
        // SAFETY: out pointers live for the calls.
        unsafe {
            s.seek(
                -1,
                IBStream_::IStreamSeekMode_::kIBSeekEnd as int32,
                &mut pos,
            );
            assert_eq!(pos, 3);
            s.seek(
                -9,
                IBStream_::IStreamSeekMode_::kIBSeekCur as int32,
                &mut pos,
            );
            assert_eq!(pos, 0);
            assert_eq!(s.seek(0, 9, &mut pos), kInvalidArgument);
            s.seek(
                2,
                IBStream_::IStreamSeekMode_::kIBSeekSet as int32,
                &mut pos,
            );
            s.tell(&mut pos);
            assert_eq!(pos, 2);
        }
    }

    #[test]
    fn hosts_real_plugins_when_available() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_VST3") else {
            eprintln!("AUDIOSTRADA_TEST_VST3 not set, skipping");
            return;
        };
        let files = crate::scan::find_files(Format::Vst3, &[dir.into()]);
        let mut hosted = 0;
        for file in files {
            for descriptor in scan(&file)
                .unwrap()
                .into_iter()
                .filter(|d| !d.instrument)
                .take(3)
            {
                let mut instance = Vst3Instance::new(&descriptor).unwrap();
                let params = instance.params();
                let mut processor = instance.activate(48_000, 256).unwrap();
                let (mut left, mut right) = (vec![0.5f32; 256], vec![0.5f32; 256]);
                let first = params
                    .iter()
                    .find(|p| !p.read_only)
                    .map(|p| (p.id, p.default));
                for _ in 0..20 {
                    processor.process(&mut left, &mut right, first.as_slice(), &[], &mut |_, _| {});
                }
                assert!(
                    left.iter().chain(&right).all(|s| s.is_finite()),
                    "{}",
                    descriptor.name
                );
                let state = instance.save_state().expect("state");
                assert!(instance.load_state(&state), "{}", descriptor.name);
                let text = first.and_then(|(id, v)| instance.value_text(id, v));
                assert!(!instance.try_deactivate());
                drop(processor);
                assert!(instance.try_deactivate());
                eprintln!(
                    "hosted {} ({} params, state {} bytes, {:?})",
                    descriptor.name,
                    params.len(),
                    state.len(),
                    text
                );
                hosted += 1;
            }
        }
        assert!(hosted > 0);
    }
}
