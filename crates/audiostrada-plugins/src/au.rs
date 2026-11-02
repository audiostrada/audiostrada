//! Audio Unit hosting on macOS: components listed from their bundle's Info.plist, run through
//! AudioToolbox, editors (the plugin's Cocoa view, or Apple's generic one) in AppKit windows.
//!
//! A plugin is identified by its type, subtype and manufacturer codes: `aufx:Rvb1:Acme`.

use std::ffi::{CStr, c_void};
use std::path::Path;
use std::ptr::{NonNull, null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};
use objc2_audio_toolbox::{
    AURenderCallbackStruct, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioUnit, AudioUnitCocoaViewInfo,
    AudioUnitGetParameter, AudioUnitGetProperty, AudioUnitGetPropertyInfo, AudioUnitInitialize,
    AudioUnitParameterInfo, AudioUnitParameterOptions, AudioUnitParameterStringFromValue,
    AudioUnitParameterUnit, AudioUnitRender, AudioUnitRenderActionFlags, AudioUnitReset,
    AudioUnitSetParameter, AudioUnitSetProperty, AudioUnitUninitialize, MusicDeviceMIDIEvent,
    kAudioUnitProperty_ClassInfo, kAudioUnitProperty_CocoaUI, kAudioUnitProperty_Latency,
    kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitProperty_OfflineRender,
    kAudioUnitProperty_ParameterInfo, kAudioUnitProperty_ParameterList,
    kAudioUnitProperty_ParameterStringFromValue, kAudioUnitProperty_SetRenderCallback,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global, kAudioUnitScope_Input,
    kAudioUnitScope_Output, kAudioUnitType_Effect, kAudioUnitType_MusicDevice,
    kAudioUnitType_MusicEffect,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked,
    kAudioFormatLinearPCM,
};
use objc2_core_foundation::{
    CFArray, CFBundle, CFData, CFDictionary, CFNumber, CFPropertyListCreateData,
    CFPropertyListCreateWithData, CFPropertyListFormat, CFRetained, CFString, CFType, CFURL,
};
use objc2_foundation::{NSRect, NSSize, NSString};

use crate::window::{HostWindow, NativeHandle};
use crate::{MidiEvent, ParamDesc, PluginDescriptor, PluginProcessor, PluginRequest};

const KINDS: [u32; 3] = [
    kAudioUnitType_Effect,
    kAudioUnitType_MusicEffect,
    kAudioUnitType_MusicDevice,
];
/// Parameter flag not named by the bindings.
const WRITABLE: u32 = 1 << 31;
/// How often parameter changes made in the plugin's window are looked for.
const WATCH: Duration = Duration::from_millis(100);

fn code(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    (bytes.len() == 4).then(|| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
fn text(code: u32) -> String {
    String::from_utf8_lossy(&code.to_be_bytes()).into_owned()
}

fn description(id: &str) -> Option<AudioComponentDescription> {
    let mut parts = id.split(':');
    let (kind, sub, maker) = (parts.next()?, parts.next()?, parts.next()?);
    Some(AudioComponentDescription {
        componentType: code(kind)?,
        componentSubType: code(sub)?,
        componentManufacturer: code(maker)?,
        componentFlags: 0,
        componentFlagsMask: 0,
    })
}

/// A string-keyed view of a CoreFoundation dictionary.
fn entries(value: CFRetained<CFType>) -> Option<CFRetained<CFDictionary<CFString, CFType>>> {
    let dictionary = value.downcast::<CFDictionary>().ok()?;
    // SAFETY: plist dictionaries have string keys; values are checked when read.
    Some(unsafe { CFRetained::cast_unchecked(dictionary) })
}

fn field(dictionary: &CFDictionary<CFString, CFType>, key: &str) -> Option<CFRetained<CFType>> {
    dictionary.get(&CFString::from_str(key))
}

fn string(dictionary: &CFDictionary<CFString, CFType>, key: &str) -> Option<String> {
    let value = field(dictionary, key)?;
    Some(value.downcast_ref::<CFString>()?.to_string())
}

/// Lists the Audio Units of a `.component` bundle from its Info.plist, without loading it.
pub fn scan(path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    let url = CFURL::from_directory_path(path).ok_or("this bundle path is not valid")?;
    let info =
        CFBundle::info_dictionary_for_url(Some(&url)).ok_or("this bundle has no Info.plist")?;
    let info = entries(info.into()).ok_or("this bundle has no Info.plist")?;
    let components = field(&info, "AudioComponents")
        .and_then(|c| c.downcast::<CFArray>().ok())
        .ok_or("this bundle declares no Audio Unit")?;
    // SAFETY: elements are checked one by one below.
    let components: CFRetained<CFArray<CFType>> = unsafe { CFRetained::cast_unchecked(components) };
    let mut out = Vec::new();
    for component in components.iter() {
        let Some(component) = entries(component) else {
            continue;
        };
        let (Some(kind), Some(sub), Some(maker)) = (
            string(&component, "type"),
            string(&component, "subtype"),
            string(&component, "manufacturer"),
        ) else {
            continue;
        };
        let Some(kind_code) = code(&kind).filter(|k| KINDS.contains(k)) else {
            continue;
        };
        // Names read "Vendor: Plugin".
        let full = string(&component, "name").unwrap_or_else(|| sub.clone());
        let (vendor, name) = match full.split_once(':') {
            Some((v, n)) => (v.trim().to_owned(), n.trim().to_owned()),
            None => (maker.clone(), full.trim().to_owned()),
        };
        let version = field(&component, "version")
            .and_then(|v| v.downcast_ref::<CFNumber>().and_then(|n| n.as_i64()))
            .map(|v| format!("{}.{}.{}", (v >> 16) & 0xffff, (v >> 8) & 0xff, v & 0xff))
            .unwrap_or_default();
        let tags = field(&component, "tags")
            .and_then(|t| t.downcast::<CFArray>().ok())
            .map(|tags| {
                // SAFETY: elements are checked as strings.
                let tags: CFRetained<CFArray<CFType>> = unsafe { CFRetained::cast_unchecked(tags) };
                tags.iter()
                    .filter_map(|t| t.downcast_ref::<CFString>().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        out.push(PluginDescriptor {
            format: crate::Format::Au,
            id: format!("{kind}:{sub}:{maker}"),
            name,
            vendor,
            version,
            path: path.to_path_buf(),
            instrument: kind_code == kAudioUnitType_MusicDevice,
            tags,
        });
    }
    Ok(out)
}

/// Bytes of a property, or `None` when the unit does not have it.
fn property_size(unit: AudioUnit, id: u32, scope: u32, element: u32) -> Option<u32> {
    let mut size = 0;
    // SAFETY: plain call on a live unit.
    let status =
        unsafe { AudioUnitGetPropertyInfo(unit, id, scope, element, &mut size, null_mut()) };
    (status == 0 && size > 0).then_some(size)
}

/// Reads a fixed-size property.
fn get<T>(unit: AudioUnit, id: u32, scope: u32, element: u32, value: &mut T) -> bool {
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` holds `size` bytes of the property's type.
    unsafe {
        AudioUnitGetProperty(
            unit,
            id,
            scope,
            element,
            NonNull::from(value).cast(),
            NonNull::from(&mut size),
        ) == 0
    }
}

fn set<T>(unit: AudioUnit, id: u32, scope: u32, element: u32, value: &T) -> bool {
    // SAFETY: `value` is the property's type.
    unsafe {
        AudioUnitSetProperty(
            unit,
            id,
            scope,
            element,
            (value as *const T).cast(),
            size_of::<T>() as u32,
        ) == 0
    }
}

fn stream_format(rate: f64, channels: u32) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat
            | kAudioFormatFlagIsPacked
            | kAudioFormatFlagIsNonInterleaved,
        mBytesPerPacket: 4,
        mFramesPerPacket: 1,
        mBytesPerFrame: 4,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 32,
        mReserved: 0,
    }
}

/// Sets a stereo format on a bus, or mono when the unit refuses stereo; the channels taken.
fn set_format(unit: AudioUnit, scope: u32, rate: f64) -> Option<u32> {
    [2, 1].into_iter().find(|&channels| {
        set(
            unit,
            kAudioUnitProperty_StreamFormat,
            scope,
            0,
            &stream_format(rate, channels),
        )
    })
}

/// Main-thread side of an Audio Unit.
pub struct AuInstance {
    descriptor: PluginDescriptor,
    unit: AudioUnit,
    /// Takes audio input (effects), rather than only MIDI (instruments).
    effect: bool,
    initialized: bool,
    rate: f64,
    processor_alive: Arc<AtomicBool>,
    /// Last values seen, to report changes made in the plugin's window.
    values: Vec<(u32, f32)>,
    watched: Instant,
    latency: u32,
    editor: Option<Editor>,
}

impl std::fmt::Debug for AuInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuInstance")
            .field("plugin", &self.descriptor.id)
            .finish()
    }
}

struct Editor {
    view: Retained<AnyObject>,
    window: HostWindow,
}

impl AuInstance {
    pub fn new(descriptor: &PluginDescriptor) -> Result<Self, String> {
        let wanted = description(&descriptor.id)
            .ok_or_else(|| format!("not an AU identifier: {}", descriptor.id))?;
        // SAFETY: a valid description, searched from the start of the registry.
        let component = unsafe { AudioComponentFindNext(null_mut(), NonNull::from(&wanted)) };
        if component.is_null() {
            return Err(format!("the Audio Unit {} is not installed", descriptor.id));
        }
        let mut unit: AudioUnit = null_mut();
        // SAFETY: a component returned by the registry.
        let status = unsafe { AudioComponentInstanceNew(component, NonNull::from(&mut unit)) };
        if status != 0 || unit.is_null() {
            return Err(format!("the Audio Unit could not start (error {status})"));
        }
        Ok(Self {
            descriptor: descriptor.clone(),
            unit,
            effect: wanted.componentType != kAudioUnitType_MusicDevice,
            initialized: false,
            rate: 48_000.0,
            processor_alive: Arc::new(AtomicBool::new(false)),
            values: Vec::new(),
            watched: Instant::now(),
            latency: 0,
            editor: None,
        })
    }

    fn parameter_ids(&self) -> Vec<u32> {
        let Some(size) = property_size(
            self.unit,
            kAudioUnitProperty_ParameterList,
            kAudioUnitScope_Global,
            0,
        ) else {
            return Vec::new();
        };
        let mut ids = vec![0u32; size as usize / 4];
        let mut size = (ids.len() * 4) as u32;
        // SAFETY: `ids` holds `size` bytes.
        let status = unsafe {
            AudioUnitGetProperty(
                self.unit,
                kAudioUnitProperty_ParameterList,
                kAudioUnitScope_Global,
                0,
                NonNull::new_unchecked(ids.as_mut_ptr()).cast(),
                NonNull::from(&mut size),
            )
        };
        if status != 0 {
            return Vec::new();
        }
        ids.truncate(size as usize / 4);
        ids
    }

    fn info(&self, id: u32) -> Option<AudioUnitParameterInfo> {
        // SAFETY: a zeroed C struct is valid; the unit fills it.
        let mut info: AudioUnitParameterInfo = unsafe { std::mem::zeroed() };
        get(
            self.unit,
            kAudioUnitProperty_ParameterInfo,
            kAudioUnitScope_Global,
            id,
            &mut info,
        )
        .then_some(info)
    }

    fn value(&self, id: u32) -> Option<f32> {
        let mut value = 0.0;
        // SAFETY: plain call on a live unit.
        let status = unsafe {
            AudioUnitGetParameter(
                self.unit,
                id,
                kAudioUnitScope_Global,
                0,
                NonNull::from(&mut value),
            )
        };
        (status == 0).then_some(value)
    }

    fn snapshot(&mut self) {
        self.values = self
            .parameter_ids()
            .into_iter()
            .filter_map(|id| Some((id, self.value(id)?)))
            .collect();
    }

    fn read_latency(&self) -> u32 {
        let mut seconds = 0.0f64;
        if get(
            self.unit,
            kAudioUnitProperty_Latency,
            kAudioUnitScope_Global,
            0,
            &mut seconds,
        ) {
            (seconds.max(0.0) * self.rate).round() as u32
        } else {
            0
        }
    }

    /// The plugin's own Cocoa view, or Apple's generic parameter view.
    fn create_view(&self, size: NSSize) -> Option<Retained<AnyObject>> {
        self.cocoa_view(size).or_else(|| self.generic_view())
    }

    fn cocoa_view(&self, size: NSSize) -> Option<Retained<AnyObject>> {
        let bytes = property_size(
            self.unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
        )?;
        let mut storage = vec![0u64; (bytes as usize).div_ceil(8)];
        let mut got = bytes;
        // SAFETY: `storage` holds `bytes` bytes, aligned for the struct.
        let status = unsafe {
            AudioUnitGetProperty(
                self.unit,
                kAudioUnitProperty_CocoaUI,
                kAudioUnitScope_Global,
                0,
                NonNull::new_unchecked(storage.as_mut_ptr()).cast(),
                NonNull::from(&mut got),
            )
        };
        if status != 0 {
            return None;
        }
        // SAFETY: the unit filled an AudioUnitCocoaViewInfo; it gave us one reference to the URL
        // and to the first class name, released below.
        let (url, class) = unsafe {
            let info = &*storage.as_ptr().cast::<AudioUnitCocoaViewInfo>();
            (
                CFRetained::from_raw(info.mCocoaAUViewBundleLocation),
                CFRetained::from_raw(info.mCocoaAUViewClass[0]),
            )
        };
        let path = url.to_file_path()?;
        let class = class.to_string();
        autoreleasepool(|_| {
            let path = NSString::from_str(&path.to_string_lossy());
            let bundle_class = AnyClass::get(c"NSBundle")?;
            // SAFETY: standard NSBundle and factory calls; the factory follows AUCocoaUIBase.
            unsafe {
                let bundle: Option<Retained<AnyObject>> =
                    msg_send![bundle_class, bundleWithPath: &*path];
                let bundle = bundle?;
                let name = NSString::from_str(&class);
                let factory_class: *const AnyClass = msg_send![&bundle, classNamed: &*name];
                let factory_class = factory_class.as_ref()?;
                let factory: Option<Retained<AnyObject>> = msg_send![factory_class, new];
                let factory = factory?;
                let view = send_unit_size(
                    &factory,
                    sel!(uiViewForAudioUnit:withSize:),
                    self.unit,
                    size,
                );
                Retained::retain(view)
            }
        })
    }

    fn generic_view(&self) -> Option<Retained<AnyObject>> {
        autoreleasepool(|_| {
            let path = NSString::from_str("/System/Library/Frameworks/CoreAudioKit.framework");
            // SAFETY: standard NSBundle calls, then AUGenericView's initialiser.
            unsafe {
                let bundle_class = AnyClass::get(c"NSBundle")?;
                let bundle: Option<Retained<AnyObject>> =
                    msg_send![bundle_class, bundleWithPath: &*path];
                let loaded: bool = msg_send![&bundle?, load];
                if !loaded {
                    return None;
                }
                let class = AnyClass::get(c"AUGenericView")?;
                let allocated: *mut AnyObject = msg_send![class, alloc];
                let view = send_unit(allocated, sel!(initWithAudioUnit:), self.unit);
                Retained::from_raw(view)
            }
        })
    }
}

/// `[object selector:unit withSize:size]`, with the AudioUnit passed as a plain pointer (its
/// Objective-C type is an opaque struct pointer that `msg_send!` would not accept).
unsafe fn send_unit_size(
    object: &AnyObject,
    selector: Sel,
    unit: AudioUnit,
    size: NSSize,
) -> *mut AnyObject {
    type Send =
        unsafe extern "C-unwind" fn(*const AnyObject, Sel, *mut c_void, NSSize) -> *mut AnyObject;
    // SAFETY: objc_msgSend called with the method's exact C signature.
    unsafe {
        let send: Send = std::mem::transmute(objc2::ffi::objc_msgSend as *const ());
        send(object, selector, unit.cast(), size)
    }
}

/// `[object selector:unit]`, as [`send_unit_size`].
unsafe fn send_unit(object: *mut AnyObject, selector: Sel, unit: AudioUnit) -> *mut AnyObject {
    type Send = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut c_void) -> *mut AnyObject;
    // SAFETY: objc_msgSend called with the method's exact C signature.
    unsafe {
        let send: Send = std::mem::transmute(objc2::ffi::objc_msgSend as *const ());
        send(object, selector, unit.cast())
    }
}

/// A CoreFoundation string returned to us, as Rust text; it is released.
fn take_string(string: *const CFString) -> Option<String> {
    // SAFETY: the caller owns this reference.
    NonNull::new(string.cast_mut()).map(|s| unsafe { CFRetained::from_raw(s) }.to_string())
}

impl crate::PluginInstance for AuInstance {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        max_frames: usize,
    ) -> Result<Box<dyn PluginProcessor>, String> {
        if self.processor_alive.load(Ordering::Relaxed) {
            return Err("the plugin is still running".into());
        }
        if self.initialized {
            // SAFETY: plain call on a live unit.
            unsafe { AudioUnitUninitialize(self.unit) };
            self.initialized = false;
        }
        self.rate = f64::from(sample_rate);
        let frames = max_frames.max(1);
        set(
            self.unit,
            kAudioUnitProperty_MaximumFramesPerSlice,
            kAudioUnitScope_Global,
            0,
            &(frames as u32),
        );
        let outputs = set_format(self.unit, kAudioUnitScope_Output, self.rate)
            .ok_or("the plugin does not accept 32-bit float audio")?;
        let mut input = Box::new(Input {
            channels: [vec![0.0; frames], vec![0.0; frames]],
        });
        if self.effect {
            set_format(self.unit, kAudioUnitScope_Input, self.rate)
                .ok_or("the plugin does not accept 32-bit float input")?;
            let callback = AURenderCallbackStruct {
                inputProc: Some(pull_input),
                inputProcRefCon: (&mut *input as *mut Input).cast(),
            };
            if !set(
                self.unit,
                kAudioUnitProperty_SetRenderCallback,
                kAudioUnitScope_Input,
                0,
                &callback,
            ) {
                return Err("the plugin does not take audio input".into());
            }
        }
        // SAFETY: plain call on a live, configured unit.
        let status = unsafe { AudioUnitInitialize(self.unit) };
        if status != 0 {
            return Err(format!("the plugin could not start (error {status})"));
        }
        self.initialized = true;
        self.latency = self.read_latency();
        self.snapshot();
        self.processor_alive.store(true, Ordering::Relaxed);
        Ok(Box::new(AuProcessor {
            unit: self.unit,
            effect: self.effect,
            input: Box::into_raw(input),
            outputs: (0..outputs).map(|_| vec![0.0; frames]).collect(),
            sample_time: 0.0,
            alive: Arc::clone(&self.processor_alive),
        }))
    }

    fn try_deactivate(&mut self) -> bool {
        if self.processor_alive.load(Ordering::Relaxed) {
            return false;
        }
        if self.initialized {
            // SAFETY: plain call on a live unit, no longer rendering.
            unsafe { AudioUnitUninitialize(self.unit) };
            self.initialized = false;
        }
        true
    }

    fn params(&mut self) -> Vec<ParamDesc> {
        self.parameter_ids()
            .into_iter()
            .filter_map(|id| {
                let info = self.info(id)?;
                let flags = info.flags.0;
                let owned = flags & AudioUnitParameterOptions::Flag_CFNameRelease.0 != 0;
                let name = if flags & AudioUnitParameterOptions::Flag_HasCFNameString.0 != 0
                    && !info.cfNameString.is_null()
                {
                    if owned {
                        take_string(info.cfNameString)
                    } else {
                        // SAFETY: a string the unit keeps alive.
                        Some(unsafe { &*info.cfNameString }.to_string())
                    }
                } else {
                    None
                }
                .unwrap_or_else(|| {
                    // SAFETY: a NUL-terminated C string inside the struct.
                    unsafe { CStr::from_ptr(info.name.as_ptr()) }
                        .to_string_lossy()
                        .into_owned()
                });
                Some(ParamDesc {
                    id,
                    name,
                    module: String::new(),
                    min: f64::from(info.minValue),
                    max: f64::from(info.maxValue),
                    default: f64::from(info.defaultValue),
                    // Indexed or Boolean.
                    stepped: info.unit.0 == AudioUnitParameterUnit::Indexed.0 || info.unit.0 == 2,
                    hidden: false,
                    read_only: flags & WRITABLE == 0,
                })
            })
            .collect()
    }

    fn param_value(&mut self, id: u32) -> Option<f64> {
        self.value(id).map(f64::from)
    }

    fn value_text(&mut self, id: u32, value: f64) -> Option<String> {
        let mut value = value as f32;
        let mut request = AudioUnitParameterStringFromValue {
            inParamID: id,
            inValue: NonNull::from(&mut value),
            outString: null(),
        };
        get(
            self.unit,
            kAudioUnitProperty_ParameterStringFromValue,
            kAudioUnitScope_Global,
            0,
            &mut request,
        )
        .then(|| take_string(request.outString))
        .flatten()
    }

    fn set_param(&mut self, id: u32, value: f64) {
        if let Some(entry) = self.values.iter_mut().find(|(i, _)| *i == id) {
            entry.1 = value as f32;
        }
    }

    fn latency(&mut self) -> u32 {
        self.latency
    }

    fn save_state(&mut self) -> Option<Vec<u8>> {
        let mut plist: *const CFType = null();
        if !get(
            self.unit,
            kAudioUnitProperty_ClassInfo,
            kAudioUnitScope_Global,
            0,
            &mut plist,
        ) {
            return None;
        }
        // SAFETY: the unit handed us one reference to its state.
        let plist = unsafe { CFRetained::from_raw(NonNull::new(plist.cast_mut())?) };
        // SAFETY: a property list from the unit, written as a binary plist.
        let data = unsafe {
            CFPropertyListCreateData(
                None,
                Some(&*plist),
                CFPropertyListFormat::BinaryFormat_v1_0,
                0,
                null_mut(),
            )
        }?;
        Some(data.to_vec())
    }

    fn load_state(&mut self, data: &[u8]) -> bool {
        let data = CFData::from_bytes(data);
        // SAFETY: a binary plist written by save_state.
        let Some(plist) = (unsafe {
            CFPropertyListCreateWithData(None, Some(&*data), 0, null_mut(), null_mut())
        }) else {
            return false;
        };
        let pointer: *const CFType = CFRetained::as_ptr(&plist).as_ptr();
        let done = set(
            self.unit,
            kAudioUnitProperty_ClassInfo,
            kAudioUnitScope_Global,
            0,
            &pointer,
        );
        self.snapshot();
        done
    }

    fn poll(&mut self) -> Vec<PluginRequest> {
        let mut requests = Vec::new();
        if self.watched.elapsed() >= WATCH {
            self.watched = Instant::now();
            for i in 0..self.values.len() {
                let (id, before) = self.values[i];
                if let Some(now) = self.value(id)
                    && now != before
                {
                    self.values[i].1 = now;
                    requests.push(PluginRequest::ParamChanged(id, f64::from(now)));
                }
            }
            if self.initialized {
                let latency = self.read_latency();
                if latency != self.latency {
                    self.latency = latency;
                    requests.push(PluginRequest::LatencyChanged);
                }
            }
        }
        if let Some(editor) = &mut self.editor
            && editor.window.poll()
        {
            self.close_gui();
            requests.push(PluginRequest::GuiClosed);
        }
        requests
    }

    fn has_gui(&mut self) -> bool {
        true
    }

    fn open_gui(&mut self, title: &str) -> Result<(), String> {
        if self.editor.is_some() {
            return Ok(());
        }
        let size = NSSize::new(640.0, 480.0);
        let view = self.create_view(size).ok_or("this plugin has no window")?;
        let mut window = HostWindow::new(title, 640, 480)?;
        let NativeHandle::Cocoa(content) = window.handle() else {
            return Err("no window to show the plugin in".into());
        };
        let content = content.cast::<AnyObject>();
        // SAFETY: the content view of our window and the plugin's view, on the main thread.
        unsafe {
            let frame: NSRect = msg_send![&*view, frame];
            window.resize(
                frame.size.width.max(100.0) as u32,
                frame.size.height.max(60.0) as u32,
            );
            let _: () = msg_send![&*view, setFrameOrigin: objc2_foundation::NSPoint::new(0.0, 0.0)];
            let _: () = msg_send![content, addSubview: &*view];
        }
        self.editor = Some(Editor { view, window });
        Ok(())
    }

    fn close_gui(&mut self) {
        if let Some(editor) = self.editor.take() {
            // SAFETY: the plugin's view, detached before its window goes away.
            unsafe {
                let _: () = msg_send![&*editor.view, removeFromSuperview];
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

impl Drop for AuInstance {
    fn drop(&mut self) {
        crate::PluginInstance::close_gui(self);
        if self.processor_alive.load(Ordering::Relaxed) {
            // The audio side still renders through the unit: leave it rather than free it.
            return;
        }
        // SAFETY: the unit is no longer used anywhere.
        unsafe {
            if self.initialized {
                AudioUnitUninitialize(self.unit);
            }
            AudioComponentInstanceDispose(self.unit);
        }
    }
}

/// Audio handed to an effect when it pulls its input, filled before each render.
struct Input {
    channels: [Vec<f32>; 2],
}

/// The input callback of effects: gives them the block being processed.
unsafe extern "C-unwind" fn pull_input(
    refcon: NonNull<c_void>,
    _flags: NonNull<AudioUnitRenderActionFlags>,
    _time: NonNull<AudioTimeStamp>,
    _bus: u32,
    frames: u32,
    data: *mut AudioBufferList,
) -> i32 {
    if data.is_null() {
        return 0;
    }
    // SAFETY: refcon is the processor's Input, alive while it renders; `data` comes from the
    // unit with `mNumberBuffers` buffers.
    unsafe {
        let input = &*refcon.as_ptr().cast::<Input>();
        let count = (*data).mNumberBuffers as usize;
        let buffers = std::slice::from_raw_parts_mut((*data).mBuffers.as_mut_ptr(), count);
        for (i, buffer) in buffers.iter_mut().enumerate() {
            let source = &input.channels[i.min(1)];
            let n = (frames as usize).min(source.len());
            if buffer.mData.is_null() {
                buffer.mData = source.as_ptr().cast_mut().cast();
            } else {
                std::ptr::copy_nonoverlapping(source.as_ptr(), buffer.mData.cast::<f32>(), n);
            }
            buffer.mDataByteSize = (n * 4) as u32;
        }
    }
    0
}

/// A buffer list of two buffers, laid out as AudioBufferList with its variable tail.
#[repr(C)]
struct StereoList {
    count: u32,
    buffers: [AudioBuffer; 2],
}

/// Audio side of an Audio Unit.
pub struct AuProcessor {
    unit: AudioUnit,
    effect: bool,
    input: *mut Input,
    outputs: Vec<Vec<f32>>,
    sample_time: f64,
    alive: Arc<AtomicBool>,
}

impl std::fmt::Debug for AuProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuProcessor").finish()
    }
}

// SAFETY: the processor is used by one audio thread at a time; the unit renders on it.
unsafe impl Send for AuProcessor {}

impl PluginProcessor for AuProcessor {
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        _params_out: &mut dyn FnMut(u32, f64),
    ) {
        let n = left
            .len()
            .min(right.len())
            .min(self.outputs.first().map_or(0, Vec::len));
        if n == 0 {
            return;
        }
        // SAFETY: plain calls on the live unit, from the rendering thread.
        unsafe {
            for &(id, value) in params_in {
                AudioUnitSetParameter(self.unit, id, kAudioUnitScope_Global, 0, value as f32, 0);
            }
            for event in midi {
                let [status, a, b] = event.data;
                MusicDeviceMIDIEvent(
                    self.unit,
                    u32::from(status),
                    u32::from(a),
                    u32::from(b),
                    crate::block_time(event, n),
                );
            }
        }
        if self.effect {
            // SAFETY: the Input is ours; the unit only reads it during AudioUnitRender below.
            let input = unsafe { &mut *self.input };
            input.channels[0][..n].copy_from_slice(&left[..n]);
            input.channels[1][..n].copy_from_slice(&right[..n]);
        }
        let mut list = StereoList {
            count: self.outputs.len() as u32,
            // SAFETY: zeroed AudioBuffers are valid, filled just below.
            buffers: unsafe { std::mem::zeroed() },
        };
        for (buffer, output) in list.buffers.iter_mut().zip(&mut self.outputs) {
            *buffer = AudioBuffer {
                mNumberChannels: 1,
                mDataByteSize: (n * 4) as u32,
                mData: output.as_mut_ptr().cast(),
            };
        }
        // SAFETY: a zeroed timestamp is valid; only the sample time is used.
        let mut time: AudioTimeStamp = unsafe { std::mem::zeroed() };
        time.mSampleTime = self.sample_time;
        time.mFlags = AudioTimeStampFlags::SampleTimeValid;
        let mut flags = AudioUnitRenderActionFlags::empty();
        // SAFETY: the list describes our output buffers of `n` frames.
        let status = unsafe {
            AudioUnitRender(
                self.unit,
                &mut flags,
                NonNull::from(&mut time),
                0,
                n as u32,
                NonNull::from(&mut list).cast(),
            )
        };
        self.sample_time += n as f64;
        if status != 0 {
            if !self.effect {
                left[..n].fill(0.0);
                right[..n].fill(0.0);
            }
            return;
        }
        // The unit may have pointed the buffers at its own memory.
        // SAFETY: each buffer now holds `n` rendered frames.
        let channel = |i: usize| unsafe {
            std::slice::from_raw_parts(list.buffers[i].mData.cast::<f32>(), n)
        };
        let l = channel(0);
        let r = if list.count > 1 { channel(1) } else { l };
        left[..n].copy_from_slice(l);
        right[..n].copy_from_slice(r);
    }

    fn reset(&mut self) {
        // SAFETY: plain call on the live unit.
        unsafe { AudioUnitReset(self.unit, kAudioUnitScope_Global, 0) };
    }

    fn set_offline(&mut self, offline: bool) {
        set(
            self.unit,
            kAudioUnitProperty_OfflineRender,
            kAudioUnitScope_Global,
            0,
            &u32::from(offline),
        );
    }
}

impl Drop for AuProcessor {
    fn drop(&mut self) {
        // SAFETY: the Input came from Box::into_raw and the unit no longer renders with it.
        drop(unsafe { Box::from_raw(self.input) });
        self.alive.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_hold_the_three_codes() {
        let d = description("aufx:Rvb1:Acme").unwrap();
        assert_eq!(
            (
                text(d.componentType),
                text(d.componentSubType),
                text(d.componentManufacturer)
            ),
            ("aufx".into(), "Rvb1".into(), "Acme".into())
        );
        assert!(description("aufx:Rvb1").is_none());
        assert!(description("aufx:toolong:Acme").is_none());
    }
}
