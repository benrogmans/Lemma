//! Fallible allocation helpers for vendored bigint digits.

use std::alloc::Layout;

#[cfg(test)]
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocError;

// The forced-failure state is thread-local on purpose: each `#[test]` runs on
// its own thread, so a test arming forced allocation failures can never poison
// allocations performed concurrently by other tests in the same binary.
#[cfg(test)]
thread_local! {
    static FORCE_ALLOC_FAIL: Cell<bool> = const { Cell::new(false) };
    static FORCE_ALLOC_FAIL_REMAINING: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub fn test_force_alloc_fail(count: usize) {
    FORCE_ALLOC_FAIL.with(|force| force.set(true));
    FORCE_ALLOC_FAIL_REMAINING.with(|remaining| remaining.set(count));
}

#[cfg(test)]
pub fn test_clear_alloc_fail() {
    FORCE_ALLOC_FAIL.with(|force| force.set(false));
    FORCE_ALLOC_FAIL_REMAINING.with(|remaining| remaining.set(0));
}

fn check_forced_fail() -> Result<(), AllocError> {
    #[cfg(test)]
    {
        if FORCE_ALLOC_FAIL.with(|force| force.get()) {
            let remaining = FORCE_ALLOC_FAIL_REMAINING.with(|remaining| {
                let value = remaining.get();
                if value > 0 {
                    remaining.set(value - 1);
                }
                value
            });
            if remaining > 0 {
                return Err(AllocError);
            }
            FORCE_ALLOC_FAIL.with(|force| force.set(false));
        }
    }
    let _ = ();
    Ok(())
}

pub fn try_reserve_exact<T>(vec: &mut Vec<T>, additional: usize) -> Result<(), AllocError> {
    check_forced_fail()?;
    vec.try_reserve_exact(additional).map_err(|_| AllocError)
}

pub fn try_with_capacity<T>(capacity: usize) -> Result<Vec<T>, AllocError> {
    check_forced_fail()?;
    let mut vec = Vec::new();
    try_reserve_exact(&mut vec, capacity)?;
    Ok(vec)
}

/// Probe the global allocator for `size` bytes; dealloc immediately on success.
pub fn try_probe_alloc(size: usize) -> Result<(), AllocError> {
    if size == 0 {
        return Ok(());
    }
    check_forced_fail()?;
    let layout = Layout::from_size_align(size, 8).map_err(|_| AllocError)?;
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() {
        return Err(AllocError);
    }
    unsafe { std::alloc::dealloc(ptr, layout) };
    Ok(())
}
