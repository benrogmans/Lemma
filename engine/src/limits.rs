use crate::error::Error;
use crate::parsing::source::Source;

pub const MAX_SPEC_NAME_LENGTH: usize = 128;
pub const MAX_FACT_NAME_LENGTH: usize = 256;
pub const MAX_RULE_NAME_LENGTH: usize = 256;
pub const MAX_TYPE_NAME_LENGTH: usize = 256;

/// Validate that a name does not exceed the given character limit.
/// `kind` is a human-readable noun like "spec", "fact", "rule", or "type".
pub fn check_max_length(
    name: &str,
    limit: usize,
    kind: &str,
    source: Option<Source>,
) -> Result<(), Error> {
    if name.len() > limit {
        return Err(Error::resource_limit_exceeded(
            format!("max_{kind}_name_length"),
            format!("{limit} characters"),
            format!("{} characters", name.len()),
            format!("Shorten the {kind} name to at most {limit} characters"),
            source,
        ));
    }
    Ok(())
}

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
