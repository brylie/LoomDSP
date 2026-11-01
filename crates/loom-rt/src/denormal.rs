//! Flush-to-zero handling for denormal floats.
//!
//! Why: when a filter or reverb tail decays toward zero, floats can become
//! "denormal" (very small numbers with reduced precision). Many CPUs process
//! denormals 10 to 100 times slower, which can cause sudden CPU spikes and
//! dropouts. Setting the CPU's flush-to-zero mode treats them as zero.

/// Enables flush-to-zero (and denormals-are-zero where available) for the
/// current thread while this guard is alive. The previous mode is restored on
/// drop.
///
/// Create one at the start of each audio callback.
#[must_use = "the mode is restored when the guard is dropped"]
pub struct ScopedFlushDenormals {
    #[cfg_attr(not(any(target_arch = "x86_64", target_arch = "aarch64")), allow(dead_code))]
    previous: usize,
}

impl ScopedFlushDenormals {
    /// Enables flush-to-zero for the current thread.
    #[inline]
    pub fn new() -> Self {
        Self {
            previous: imp::enable(),
        }
    }
}

impl Default for ScopedFlushDenormals {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ScopedFlushDenormals {
    #[inline]
    fn drop(&mut self) {
        imp::restore(self.previous);
    }
}

#[cfg(target_arch = "x86_64")]
mod imp {
    // MXCSR bits: FTZ (flush to zero) = 0x8000, DAZ (denormals are zero) = 0x0040.
    const FTZ_DAZ: u32 = 0x8040;

    #[inline]
    #[allow(deprecated)]
    pub fn enable() -> usize {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        // SAFETY: SSE is always available on x86_64. Reading and writing MXCSR
        // only changes floating point behaviour of the current thread.
        unsafe {
            let previous = _mm_getcsr();
            _mm_setcsr(previous | FTZ_DAZ);
            previous as usize
        }
    }

    #[inline]
    #[allow(deprecated)]
    pub fn restore(previous: usize) {
        // SAFETY: restores a value previously read from MXCSR on this thread.
        unsafe { std::arch::x86_64::_mm_setcsr(previous as u32) }
    }
}

#[cfg(target_arch = "aarch64")]
mod imp {
    // FPCR bit 24 is FZ (flush to zero).
    const FZ: usize = 1 << 24;

    #[inline]
    pub fn enable() -> usize {
        let previous: usize;
        // SAFETY: reading and writing FPCR only changes floating point
        // behaviour of the current thread.
        unsafe {
            std::arch::asm!("mrs {}, fpcr", out(reg) previous, options(nomem, nostack, preserves_flags));
            std::arch::asm!("msr fpcr, {}", in(reg) previous | FZ, options(nomem, nostack, preserves_flags));
        }
        previous
    }

    #[inline]
    pub fn restore(previous: usize) {
        // SAFETY: restores a value previously read from FPCR on this thread.
        unsafe {
            std::arch::asm!("msr fpcr, {}", in(reg) previous, options(nomem, nostack, preserves_flags));
        }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod imp {
    // Other targets (including wasm32) have no flush-to-zero control.
    // Modules should avoid denormals on their own where it matters.
    #[inline]
    pub fn enable() -> usize {
        0
    }

    #[inline]
    pub fn restore(_previous: usize) {}
}

#[cfg(test)]
mod tests;
