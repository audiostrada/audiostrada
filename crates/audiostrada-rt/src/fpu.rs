//! Denormal numbers flushed to zero while audio is processed. Filters and reverbs decaying into
//! silence produce them, and most processors handle them many times slower than normal numbers.

/// Flushes denormals to zero on this thread until dropped, then restores the previous mode.
#[derive(Debug)]
#[must_use = "the mode is restored when the guard is dropped"]
pub struct NoDenormals {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    saved: usize,
}

#[cfg(target_arch = "x86_64")]
mod mode {
    /// Flush to zero (bit 15) and denormals are zero (bit 6) in MXCSR.
    const FLUSH: u32 = 0x8040;

    pub fn get() -> usize {
        let mut csr = 0u32;
        // SAFETY: stores MXCSR into a local; SSE is always present on x86_64.
        unsafe { core::arch::asm!("stmxcsr [{}]", in(reg) &raw mut csr, options(nostack)) };
        csr as usize
    }

    pub fn set(value: usize) {
        let csr = value as u32;
        // SAFETY: loads a value read from MXCSR with only the flush bits changed.
        unsafe { core::arch::asm!("ldmxcsr [{}]", in(reg) &raw const csr, options(nostack)) };
    }

    pub fn flushing(value: usize) -> usize {
        value | FLUSH as usize
    }
}

#[cfg(target_arch = "aarch64")]
mod mode {
    /// Flush to zero, bit 24 of FPCR.
    const FLUSH: usize = 1 << 24;

    pub fn get() -> usize {
        let fpcr: usize;
        // SAFETY: reads the floating-point control register.
        unsafe { core::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack)) };
        fpcr
    }

    pub fn set(value: usize) {
        // SAFETY: writes back a value read from FPCR with only the flush bit changed.
        unsafe { core::arch::asm!("msr fpcr, {}", in(reg) value, options(nomem, nostack)) };
    }

    pub fn flushing(value: usize) -> usize {
        value | FLUSH
    }
}

impl NoDenormals {
    pub fn new() -> Self {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            let saved = mode::get();
            let flushing = mode::flushing(saved);
            if flushing != saved {
                mode::set(flushing);
            }
            Self { saved }
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        Self {}
    }
}

impl Default for NoDenormals {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NoDenormals {
    fn drop(&mut self) {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if mode::get() != self.saved {
            mode::set(self.saved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denormals_flush_inside_the_guard_only() {
        let tiny = std::hint::black_box(f32::MIN_POSITIVE);
        assert!((tiny / 4.0).is_subnormal());
        {
            let _guard = NoDenormals::new();
            let flushed = std::hint::black_box(tiny) / std::hint::black_box(4.0);
            if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
                assert_eq!(flushed, 0.0);
            }
        }
        assert!((std::hint::black_box(tiny) / std::hint::black_box(4.0)).is_subnormal());
    }
}
