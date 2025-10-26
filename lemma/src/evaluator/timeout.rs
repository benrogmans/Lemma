//! Timeout tracking for evaluation
//!
//! Provides platform-specific timeout tracking. On native targets, uses std::time::Instant
//! to track elapsed time. On WASM, timeout checking is a no-op since std::time::Instant
//! is not available in the wasm32 target.

use crate::{LemmaError, ResourceLimits};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

/// Timeout tracker for evaluation
///
/// On native platforms, tracks actual elapsed time using Instant.
/// On WASM, this is a zero-cost abstraction with no-op timeout checks.
pub struct TimeoutTracker {
    #[cfg(not(target_arch = "wasm32"))]
    start_time: Instant,
}

impl TimeoutTracker {
    /// Create a new timeout tracker
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
    }

    /// Create a new timeout tracker (WASM version)
    #[cfg(target_arch = "wasm32")]
    pub fn new() -> Self {
        Self {}
    }

    /// Check if evaluation has exceeded the timeout limit
    ///
    /// On native platforms, returns an error if elapsed time exceeds max_evaluation_time_ms.
    /// On WASM, always returns Ok (timeout checking not available).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn check_timeout(&self, limits: &ResourceLimits) -> Result<(), LemmaError> {
        let elapsed_ms = self.start_time.elapsed().as_millis() as u64;
        if elapsed_ms > limits.max_evaluation_time_ms {
            return Err(LemmaError::ResourceLimitExceeded {
                limit_name: "max_evaluation_time_ms".to_string(),
                limit_value: limits.max_evaluation_time_ms.to_string(),
                actual_value: elapsed_ms.to_string(),
                suggestion: format!(
                    "Evaluation took {}ms, exceeding the limit of {}ms. Simplify the document or increase the timeout.",
                    elapsed_ms, limits.max_evaluation_time_ms
                ),
            });
        }
        Ok(())
    }

    /// Check if evaluation has exceeded the timeout limit (WASM version - no-op)
    #[cfg(target_arch = "wasm32")]
    pub fn check_timeout(&self, _limits: &ResourceLimits) -> Result<(), LemmaError> {
        // Timeout checking not available on WASM (no std::time::Instant)
        Ok(())
    }
}

impl Default for TimeoutTracker {
    fn default() -> Self {
        Self::new()
    }
}
