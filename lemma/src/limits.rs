/// Limits to prevent abuse and enable predictable resource usage
///
/// These limits protect against malicious inputs while being generous enough
/// for all legitimate use cases.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum file size in bytes
    /// Real usage: ~5KB, Limit: 5MB (1000x)
    pub max_file_size_bytes: usize,

    /// Maximum expression nesting depth
    /// Real usage: ~3 levels, Limit: 100 (30x+)
    pub max_expression_depth: usize,

    /// Maximum size of a single fact value in bytes
    /// Real usage: ~100 bytes, Limit: 1KB (10x)
    /// Enables server pre-allocation for zero-allocation evaluation
    pub max_fact_value_bytes: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_file_size_bytes: 5 * 1024 * 1024, // 5 MB
            max_expression_depth: 100,
            max_fact_value_bytes: 1024, // 1 KB
        }
    }
}

impl ResourceLimits {
    /// Create a new ResourceLimits with default values
    pub fn new() -> Self {
        Self::default()
    }
}
