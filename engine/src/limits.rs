use crate::error::Error;
use crate::parsing::source::Source;

pub const MAX_SPEC_NAME_LENGTH: usize = 128;
pub const MAX_DATA_NAME_LENGTH: usize = 256;
pub const MAX_RULE_NAME_LENGTH: usize = 256;

/// Maximum character length for a text value (data/runtime input).
pub const MAX_TEXT_VALUE_LENGTH: usize = 1024;

/// Validate that a name does not exceed the given character limit.
/// `kind` is a human-readable noun like "spec", "data", "rule", or "type".
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
            None,
            None,
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
    /// Maximum size of one loaded source text in bytes.
    /// Real usage: ~5KB, Limit: 5MB (1000x)
    pub max_source_size_bytes: usize,

    /// Maximum expression nesting depth
    /// Real usage: ~3 levels, Limit: 7. Deeper logic via rule composition.
    pub max_expression_depth: usize,

    /// Maximum expression nodes per source (parser-level)
    /// Quick-reject for pathological single sources.
    pub max_expression_count: usize,

    /// Maximum total expression nodes across all sources (engine-level)
    /// The real capacity ceiling. pi (~3.1M) — generous for national-scale
    /// regulatory systems while bounding total engine workload.
    pub max_total_expression_count: usize,

    /// Maximum size of a single data value in bytes
    /// Real usage: ~100 bytes, Limit: 1KB (10x)
    /// Enables server pre-allocation for zero-allocation evaluation
    pub max_data_value_bytes: usize,

    /// Maximum total bytes to load in one batch (and/or in-memory size of loaded specs)
    pub max_loaded_bytes: usize,

    /// Maximum number of sources in one load batch (e.g. after expanding paths on disk)
    pub max_sources: usize,

    /// Maximum expression nodes for one rule after transitive rule inlining
    /// during planning. Inlining materializes shared subtrees, so a short
    /// chain of self-doubling rules grows exponentially; this limit rejects
    /// such chains with a planning error before any tree is materialized.
    ///
    /// The default is chosen so the compiled instruction operands always fit
    /// `u16`: compilation allocates at most two registers (and at most one
    /// constant/data/veto table entry) per node, and normalization passes
    /// grow the tree by at most a small constant factor, so 30,000 nodes
    /// stays well below the 65,535 register ceiling.
    pub max_normalized_expression_nodes: usize,

    /// Maximum depth of the spec dependency chain (`uses` imports) from the
    /// root spec. Bounds recursion in dependency discovery and graph building.
    /// Real usage: ~3 levels, Limit: 32 (10x).
    pub max_spec_dependency_depth: usize,

    /// Maximum number of specs in one dependency DAG (the root spec plus all
    /// transitive dependencies). Bounds per-plan memory and planning work.
    pub max_dag_specs: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_source_size_bytes: 5 * 1024 * 1024, // 5 MB
            max_expression_depth: 7,
            max_expression_count: 65_536,
            max_total_expression_count: 3_141_592,
            max_data_value_bytes: 1024,         // 1 KB
            max_loaded_bytes: 50 * 1024 * 1024, // 50 MB
            max_sources: 4096,
            max_normalized_expression_nodes: 30_000,
            max_spec_dependency_depth: 32,
            max_dag_specs: 4096,
        }
    }
}
