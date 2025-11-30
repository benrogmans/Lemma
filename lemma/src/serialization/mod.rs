//! Serialization module for converting external data formats to Lemma values.
//!
//! These functions convert data from external formats (JSON, MsgPack, Protobuf)
//! into typed `LiteralValue`s ready for use with `ExecutionPlan::with_typed_values()`.
//!
//! The serializers are flexible about input formats while being strict about output types:
//! - **Text facts** accept any JSON type (strings pass through, others serialize to JSON)
//! - **Number facts** accept JSON numbers or parseable strings
//! - **Percentage facts** accept JSON numbers or strings (with or without %)
//! - **Boolean facts** accept JSON booleans or keyword strings
//! - **null values** are skipped (treated as "fact not provided")
//!
//! # Example
//!
//! ```ignore
//! use lemma::serialization::from_json;
//!
//! let json = br#"{"discount": 21, "config": {"key": "value"}, "name": null}"#;
//! let values = from_json(json, &execution_plan)?;
//! // discount -> Percentage(21)
//! // config -> Text("{\"key\":\"value\"}") (if config is a text fact)
//! // name -> skipped (null)
//! let plan = execution_plan.with_typed_values(values, &limits)?;
//! ```

mod json;
mod msgpack;
mod protobuf;

pub use json::from_json;
pub use json::{serialize_literal_value, serialize_operation_result};
pub use msgpack::from_msgpack;
pub use protobuf::from_protobuf;
