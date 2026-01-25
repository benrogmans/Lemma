use crate::planning::ExecutionPlan;
use crate::LemmaError;
use std::collections::HashMap;

/// Convert MsgPack values to string values for use with ExecutionPlan::with_values().
///
/// This is a stub implementation.
pub fn from_msgpack(
    _msgpack: &[u8],
    _plan: &ExecutionPlan,
) -> Result<HashMap<String, String>, LemmaError> {
    todo!("MsgPack serialization not yet implemented");
}
