//! Fallible arbitrary-precision integers vendored from num-bigint 0.4.6 algorithms.
//!
//! See LICENSE in this directory for upstream attribution.

mod alloc;
mod biguint;
mod digit;
mod signed;

pub use alloc::AllocError;
pub use signed::BigInt;

#[cfg(test)]
pub use alloc::{test_clear_alloc_fail, test_force_alloc_fail};
