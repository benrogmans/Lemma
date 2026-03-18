//! Serialization: Lemma values ↔ JSON.
//!
//! **Input (deserialization):** JSON → string fact values for evaluation.
//!
//! - [`from_json`] parses JSON and converts each value to a string for
//!   `ExecutionPlan::with_values()`.
//! - null values are skipped (treated as "fact not provided").
//!
//! **Output (serialization):** Lemma evaluation results → JSON.
//!
//! - [`literal_value_to_json`] converts a `LiteralValue` to `(serde_json::Value, Option<unit>)`
//!   for use in API/CLI responses.
//!
//! # Example (input)
//!
//! ```ignore
//! use lemma::serialization::from_json;
//!
//! let json = br#"{"discount": 21, "config": {"key": "value"}, "name": null}"#;
//! let values = from_json(json)?;
//! let plan = execution_plan.with_values(values, &limits)?;
//! ```
//!
//! # Example (output)
//!
//! ```ignore
//! use lemma::serialization::literal_value_to_json;
//!
//! let (json_value, unit) = literal_value_to_json(&literal_value);
//! ```

mod json;

pub use json::literal_value_to_json;
pub use json::{
    deserialize_fact_path_set, deserialize_resolved_fact_value_map, serialize_fact_path_set,
    serialize_resolved_fact_value_map,
};
pub use json::{fact_values_from_map, from_json};
