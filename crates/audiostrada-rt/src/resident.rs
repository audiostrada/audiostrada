//! Keeps memory that the audio thread reads in RAM. Audio decoded at load is read once per pass
//! of the playhead: under memory pressure the system compresses or swaps it out meanwhile, and
//! the audio thread then waits for the disk in the middle of a cycle, which the device plays as
//! a gap. Locked pages are never paged out.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Bytes locked by every [`Resident`] alive.
static LOCKED: AtomicUsize = AtomicUsize::new(0);

/// Pages locked for as long as it lives; unlocked when dropped.
#[derive(Debug)]
pub struct Resident {
    ranges: Vec<(usize, usize)>,
    bytes: usize,
}

/// Locks `slices` in RAM, within a quarter of the machine's memory for all of them together:
/// beyond it, or where the system refuses (a low lock limit), they stay pageable as before.
/// `None` when nothing could be locked.
pub fn keep_resident(slices: &[&[f32]]) -> Option<Resident> {
    keep_within(slices, physical_memory() / 4)
}

fn keep_within(slices: &[&[f32]], budget: usize) -> Option<Resident> {
    let bytes: usize = slices.iter().map(|s| std::mem::size_of_val(*s)).sum();
    if bytes == 0 {
        return None;
    }
    let before = LOCKED.fetch_add(bytes, Ordering::Relaxed);
    if before + bytes > budget {
        LOCKED.fetch_sub(bytes, Ordering::Relaxed);
        OVER_BUDGET.store(true, Ordering::Relaxed);
        note_refused("over the budget for locked audio");
        return None;
    }
    let mut resident = Resident {
        ranges: Vec::new(),
        bytes,
    };
    for slice in slices.iter().filter(|s| !s.is_empty()) {
        let range = (slice.as_ptr() as usize, std::mem::size_of_val(*slice));
        if !lock(range) {
            note_refused("refused by the system");
            // Dropping unlocks what was locked and gives the whole budget back.
            return None;
        }
        resident.ranges.push(range);
    }
    Some(resident)
}

impl Drop for Resident {
    fn drop(&mut self) {
        for &range in &self.ranges {
            unlock(range);
        }
        LOCKED.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

/// Bytes currently locked.
pub fn resident_bytes() -> usize {
    LOCKED.load(Ordering::Relaxed)
}

static OVER_BUDGET: AtomicBool = AtomicBool::new(false);

/// Whether audio was left pageable since the last call because it is larger than the share of
/// memory it may lock (not because the system refuses).
pub fn take_over_budget() -> bool {
    OVER_BUDGET.swap(false, Ordering::Relaxed)
}

fn note_refused(why: &str) {
    static TOLD: AtomicBool = AtomicBool::new(false);
    if !TOLD.swap(true, Ordering::Relaxed) {
        tracing::info!("audio not locked in memory ({why}): it may be paged out");
    }
}

#[cfg(unix)]
fn physical_memory() -> usize {
    // SAFETY: plain queries of system constants.
    let (pages, size) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    if pages <= 0 || size <= 0 {
        return 0;
    }
    (pages as usize).saturating_mul(size as usize)
}

#[cfg(unix)]
fn lock((start, len): (usize, usize)) -> bool {
    // SAFETY: the range is a live slice; locking only pins its pages.
    unsafe { libc::mlock(start as *const libc::c_void, len) == 0 }
}

#[cfg(unix)]
fn unlock((start, len): (usize, usize)) {
    // SAFETY: as in `lock`; unlocking a range never touches its contents.
    unsafe {
        libc::munlock(start as *const libc::c_void, len);
    }
}

#[cfg(windows)]
fn physical_memory() -> usize {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is a valid MEMORYSTATUSEX with its length set.
    match unsafe { GlobalMemoryStatusEx(&mut status) } {
        Ok(()) => usize::try_from(status.ullTotalPhys).unwrap_or(usize::MAX),
        Err(_) => 0,
    }
}

/// Windows locks pages within the process's working set only: it grows by what is locked.
#[cfg(windows)]
fn grow_working_set(by: isize) -> bool {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessWorkingSetSize, SetProcessWorkingSetSize,
    };
    let (mut min, mut max) = (0usize, 0usize);
    // SAFETY: the pseudo handle of the current process and two valid out pointers.
    unsafe {
        let process = GetCurrentProcess();
        if GetProcessWorkingSetSize(process, &mut min, &mut max).is_err() {
            return false;
        }
        let min = min.saturating_add_signed(by);
        let max = max.saturating_add_signed(by).max(min);
        SetProcessWorkingSetSize(process, min, max).is_ok()
    }
}

#[cfg(windows)]
fn lock((start, len): (usize, usize)) -> bool {
    use windows::Win32::System::Memory::VirtualLock;
    if !grow_working_set(len as isize) {
        return false;
    }
    // SAFETY: the range is a live slice; locking only pins its pages.
    let locked = unsafe { VirtualLock(start as *const std::ffi::c_void, len) }.is_ok();
    if !locked {
        grow_working_set(-(len as isize));
    }
    locked
}

#[cfg(windows)]
fn unlock((start, len): (usize, usize)) {
    use windows::Win32::System::Memory::VirtualUnlock;
    // SAFETY: as in `lock`; unlocking a range never touches its contents.
    let _ = unsafe { VirtualUnlock(start as *const std::ffi::c_void, len) };
    grow_working_set(-(len as isize));
}

#[cfg(not(any(unix, windows)))]
fn physical_memory() -> usize {
    0
}

#[cfg(not(any(unix, windows)))]
fn lock(_: (usize, usize)) -> bool {
    false
}

#[cfg(not(any(unix, windows)))]
fn unlock(_: (usize, usize)) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn decoded_audio_is_locked_and_unlocked_with_its_guard() {
        let audio = vec![0.25f32; 1 << 20];
        let before = resident_bytes();
        let resident = keep_resident(&[&audio]).expect("a few megabytes are locked");
        assert_eq!(resident_bytes(), before + (4 << 20));
        drop(resident);
        assert_eq!(resident_bytes(), before);
    }

    #[test]
    fn nothing_is_locked_beyond_the_budget() {
        let audio = vec![0.0f32; 1024];
        let before = resident_bytes();
        assert!(keep_within(&[&audio], before + 1024).is_none());
        assert_eq!(resident_bytes(), before);
        assert!(take_over_budget());
    }
}
