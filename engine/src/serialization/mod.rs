//! Serialization: Lemma values ↔ JSON.
//!
//! **Input:** [`from_json`] / [`data_values_from_map`] produce `serde_json::Value` maps for
//! [`ExecutionPlan::set_data_values`]. Convenience strings, JSON numbers, and serialized objects
//! are accepted on input. Use [`data_values_from_strings`] for CLI-style string maps. Output
//! keeps numbers as JSON strings.
//!
//! **Output:** [`ValueKind`] serialization (in `planning::semantics`) is used everywhere, including
//! evaluation responses.

mod json;

pub use json::{
    data_values_from_map, data_values_from_strings, deserialize_resolved_data_value_map, from_json,
    serialize_resolved_data_value_map,
};
