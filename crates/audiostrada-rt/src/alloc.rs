//! Detection of heap (de)allocations inside real-time sections.
//!
//! Checks are active once the binary installs the checker with
//! [`install_alloc_checker!`](crate::install_alloc_checker), which only happens in debug builds.

pub use assert_no_alloc::{AllocDisabler, permit_alloc};

/// Runs `f` as real-time code: any allocation inside it is counted as a violation.
#[inline]
pub fn rt_section<T>(f: impl FnOnce() -> T) -> T {
    assert_no_alloc::assert_no_alloc(f)
}

/// Number of allocations detected so far in real-time sections of the current thread.
#[inline]
pub fn alloc_violations() -> u32 {
    assert_no_alloc::violation_count()
}

/// Installs the allocation checker as the global allocator in debug builds.
#[macro_export]
macro_rules! install_alloc_checker {
    () => {
        #[cfg(debug_assertions)]
        #[global_allocator]
        static AUDIOSTRADA_ALLOC_CHECKER: $crate::alloc::AllocDisabler =
            $crate::alloc::AllocDisabler;
    };
}
