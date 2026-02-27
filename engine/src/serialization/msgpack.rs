use crate::planning::ExecutionPlan;
use crate::Error;
use std::collections::HashMap;

/// Convert MsgPack values to string values for use with ExecutionPlan::with_values().
pub fn from_msgpack(
    _msgpack: &[u8],
    _plan: &ExecutionPlan,
) -> Result<HashMap<String, String>, Error> {
    todo!("MsgPack serialization not yet implemented");
}
