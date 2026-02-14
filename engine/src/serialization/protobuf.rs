use crate::planning::ExecutionPlan;
use crate::LemmaError;
use std::collections::HashMap;

/// Convert Protobuf values to string values for use with ExecutionPlan::with_values().
pub fn from_protobuf(
    _protobuf: &[u8],
    _plan: &ExecutionPlan,
) -> Result<HashMap<String, String>, LemmaError> {
    todo!("Protobuf serialization not yet implemented");
}
