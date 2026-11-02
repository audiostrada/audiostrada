//! Shared memory between AudioStrada and a plugin host process, and wake-ups across processes on
//! counters stored in it (futex on Linux, ulock on macOS, named events on Windows).

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// A mapped region; the creator removes it on drop.
pub struct Shared {
    ptr: *mut u8,
    len: usize,
    name: String,
    owner: bool,
    #[cfg(windows)]
    handles: Vec<windows::Win32::Foundation::HANDLE>,
}

// SAFETY: the region is plain memory accessed through atomics and fixed buffers.
unsafe impl Send for Shared {}
// SAFETY: as above.
unsafe impl Sync for Shared {}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("name", &self.name)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

/// A unique name for a new region.
pub fn unique_name() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!("audiostrada-{}-{n}-{nanos:x}", std::process::id())
}

#[cfg(unix)]
fn path(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new("/dev/shm");
    if cfg!(target_os = "linux") && dir.is_dir() {
        dir.join(name)
    } else {
        std::env::temp_dir().join(name)
    }
}

impl Shared {
    #[cfg(unix)]
    fn map(name: &str, len: usize, create: bool) -> Result<Self, String> {
        use std::os::fd::AsRawFd;
        let path = path(name);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(create)
            .open(&path)
            .map_err(|e| format!("shared memory {}: {e}", path.display()))?;
        // A region left half made would stay in /dev/shm.
        let failed = |e: String| {
            if create {
                let _ = std::fs::remove_file(&path);
            }
            e
        };
        if create {
            file.set_len(len as u64)
                .map_err(|e| failed(e.to_string()))?;
        } else if file.metadata().map_or(0, |m| m.len()) < len as u64 {
            // Touching a mapping past the end of its file would kill the process.
            return Err(format!("shared memory {} is too small", path.display()));
        }
        // SAFETY: maps `len` bytes of a file of at least that size; unmapped on drop.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(failed(std::io::Error::last_os_error().to_string()));
        }
        Ok(Self {
            ptr: ptr.cast(),
            len,
            name: name.to_owned(),
            owner: create,
        })
    }

    #[cfg(windows)]
    fn map(name: &str, len: usize, create: bool) -> Result<Self, String> {
        use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows::Win32::System::Memory::*;
        let wide = wide(&format!("Local\\{name}"));
        // SAFETY: plain Win32 calls; the name is NUL-terminated and outlives them.
        unsafe {
            let handle = if create {
                CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    None,
                    PAGE_READWRITE,
                    (len as u64 >> 32) as u32,
                    len as u32,
                    windows::core::PCWSTR(wide.as_ptr()),
                )
            } else {
                OpenFileMappingW(
                    FILE_MAP_ALL_ACCESS.0,
                    false,
                    windows::core::PCWSTR(wide.as_ptr()),
                )
            }
            .map_err(|e| e.to_string())?;
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, len);
            if view.Value.is_null() {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return Err("cannot map the shared memory".into());
            }
            Ok(Self {
                ptr: view.Value.cast(),
                len,
                name: name.to_owned(),
                owner: create,
                handles: vec![handle],
            })
        }
    }

    pub fn create(name: &str, len: usize) -> Result<Self, String> {
        Self::map(name, len, true)
    }

    pub fn open(name: &str, len: usize) -> Result<Self, String> {
        Self::map(name, len, false)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }

    /// A wake-up channel on the counter at `offset`; `tag` names its Windows event.
    pub fn signal(&self, offset: usize, tag: &str) -> Result<Signal, String> {
        assert!(offset + 4 <= self.len && offset.is_multiple_of(4));
        // SAFETY: in bounds and aligned (checked above); the region outlives the signal's use.
        let counter = unsafe { &*(self.ptr.add(offset) as *const AtomicU32) };
        Signal::new(counter, &format!("{}-{tag}", self.name), self.owner)
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: unmaps the region mapped in `map`.
        unsafe {
            libc::munmap(self.ptr.cast(), self.len);
            if self.owner {
                let _ = std::fs::remove_file(path(&self.name));
            }
        }
        #[cfg(windows)]
        // SAFETY: unmaps the view and closes the handles opened in `map`.
        unsafe {
            let view = windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr.cast(),
            };
            let _ = windows::Win32::System::Memory::UnmapViewOfFile(view);
            for h in &self.handles {
                let _ = windows::Win32::Foundation::CloseHandle(*h);
            }
        }
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Waits for and announces changes of a counter in shared memory.
pub struct Signal {
    counter: *const AtomicU32,
    #[cfg(windows)]
    event: windows::Win32::Foundation::HANDLE,
}

// SAFETY: the counter is an atomic in shared memory; the event handle is thread-safe.
unsafe impl Send for Signal {}
// SAFETY: as above.
unsafe impl Sync for Signal {}

impl std::fmt::Debug for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signal").finish_non_exhaustive()
    }
}

impl Signal {
    #[cfg(unix)]
    fn new(counter: &AtomicU32, _name: &str, _create: bool) -> Result<Self, String> {
        Ok(Self { counter })
    }

    #[cfg(windows)]
    fn new(counter: &AtomicU32, name: &str, create: bool) -> Result<Self, String> {
        use windows::Win32::System::Threading::*;
        let wide = wide(&format!("Local\\{name}"));
        let name = windows::core::PCWSTR(wide.as_ptr());
        // SAFETY: plain Win32 calls with a NUL-terminated name.
        let event = unsafe {
            if create {
                CreateEventW(None, false, false, name)
            } else {
                OpenEventW(
                    EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                    false,
                    name,
                )
            }
        }
        .map_err(|e| e.to_string())?;
        Ok(Self { counter, event })
    }

    pub fn counter(&self) -> &AtomicU32 {
        // SAFETY: points into a mapping that outlives the signal.
        unsafe { &*self.counter }
    }

    /// Stores `value` and wakes the other side.
    pub fn set(&self, value: u32) {
        self.counter().store(value, Ordering::Release);
        self.wake();
    }

    /// Waits while the counter equals `value`, at most `timeout`; true once it changed.
    pub fn wait_while(&self, value: u32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        // A short spin catches fast answers without a system call.
        for _ in 0..256 {
            if self.counter().load(Ordering::Acquire) != value {
                return true;
            }
            std::hint::spin_loop();
        }
        loop {
            if self.counter().load(Ordering::Acquire) != value {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            self.sleep(value, deadline - now);
        }
    }

    #[cfg(target_os = "linux")]
    fn sleep(&self, value: u32, timeout: Duration) {
        let ts = libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        // SAFETY: shared (not private) futex wait on a live atomic, with a relative timeout.
        unsafe {
            libc::syscall(
                libc::SYS_futex,
                self.counter,
                libc::FUTEX_WAIT,
                value,
                &ts as *const libc::timespec,
            );
        }
    }

    #[cfg(target_os = "linux")]
    pub fn wake(&self) {
        // SAFETY: shared futex wake on a live atomic.
        unsafe {
            libc::syscall(libc::SYS_futex, self.counter, libc::FUTEX_WAKE, i32::MAX);
        }
    }

    #[cfg(target_os = "macos")]
    fn sleep(&self, value: u32, timeout: Duration) {
        let micros = timeout.as_micros().clamp(1, u128::from(u32::MAX)) as u32;
        // SAFETY: compare-and-wait on a live atomic shared between processes.
        unsafe {
            __ulock_wait(
                UL_COMPARE_AND_WAIT_SHARED,
                self.counter as *mut libc::c_void,
                u64::from(value),
                micros,
            );
        }
    }

    #[cfg(target_os = "macos")]
    pub fn wake(&self) {
        // SAFETY: wakes every waiter on a live atomic.
        unsafe {
            __ulock_wake(
                UL_COMPARE_AND_WAIT_SHARED | ULF_WAKE_ALL,
                self.counter as *mut libc::c_void,
                0,
            );
        }
    }

    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    fn sleep(&self, _value: u32, timeout: Duration) {
        std::thread::sleep(timeout.min(Duration::from_micros(200)));
    }

    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    pub fn wake(&self) {}

    #[cfg(windows)]
    fn sleep(&self, _value: u32, timeout: Duration) {
        let ms = timeout.as_millis().clamp(1, u128::from(u32::MAX - 1)) as u32;
        // SAFETY: waits on our event handle.
        unsafe {
            windows::Win32::System::Threading::WaitForSingleObject(self.event, ms);
        }
    }

    #[cfg(windows)]
    pub fn wake(&self) {
        // SAFETY: signals our event handle.
        unsafe {
            let _ = windows::Win32::System::Threading::SetEvent(self.event);
        }
    }
}

#[cfg(windows)]
impl Drop for Signal {
    fn drop(&mut self) {
        // SAFETY: closes the handle opened in `new`.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.event);
        }
    }
}

#[cfg(target_os = "macos")]
const UL_COMPARE_AND_WAIT_SHARED: u32 = 3;
#[cfg(target_os = "macos")]
const ULF_WAKE_ALL: u32 = 0x100;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn __ulock_wait(operation: u32, addr: *mut libc::c_void, value: u64, timeout_us: u32) -> i32;
    fn __ulock_wake(operation: u32, addr: *mut libc::c_void, wake_value: u64) -> i32;
}
