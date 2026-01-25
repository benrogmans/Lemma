//! Serialization module for converting external data formats to string values.
//!
//! These functions convert data from external formats (JSON, MsgPack, Protobuf)
//! into string values ready for use with `ExecutionPlan::with_values()`.
//!
//! - **null values** are skipped (treated as "fact not provided")
//! - JSON numbers, booleans, arrays, objects are converted to their string representation
//!
//! # Example
//!
//! ```ignore
//! use lemma::serialization::from_json;
//!
//! let json = br#"{"discount": 21, "config": {"key": "value"}, "name": null}"#;
//! let values = from_json(json, &execution_plan)?;
//! // discount -> "21"
//! // config -> "{\"key\":\"value\"}"
//! // name -> skipped (null)
//! let plan = execution_plan.with_values(values, &limits)?;
//! ```

mod json;
mod msgpack;
mod protobuf;

pub use json::from_json;
pub use json::{
    deserialize_fact_doc_ref_map, deserialize_fact_path_map, deserialize_fact_path_set,
    deserialize_fact_source_map, deserialize_fact_type_map, deserialize_fact_value_map,
    serialize_fact_doc_ref_map, serialize_fact_path_map, serialize_fact_path_set,
    serialize_fact_source_map, serialize_fact_type_map, serialize_fact_value_map,
};
pub use msgpack::from_msgpack;
pub use protobuf::from_protobuf;
