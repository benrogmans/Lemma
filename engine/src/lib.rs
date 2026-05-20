//! # Lemma Engine
//!
//! **Rules for man and machine**
//!
//! Lemma is a declarative programming language for expressing rules, data, and business logic
//! in a way that is both human-readable and machine-executable.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use lemma::{Engine, SourceType};
//!
//! let mut engine = Engine::new();
//!
//! // Load Lemma code
//! engine.load(r#"
//!     spec example
//!     data price: 100
//!     data quantity: 5
//!     rule total: price * quantity
//! "#, SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("example.lemma")))).expect("failed to load");
//!
//! // Evaluate the spec (all rules, no data values)
//! let now = lemma::DateTimeValue::now();
//! let response = engine.run(None, "example", Some(&now), std::collections::HashMap::new(), false, lemma::EvaluationRequest::default()).unwrap();
//! ```
//!
//! ## Core Concepts
//!
//! ### Specs
//! A spec is a collection of data and rules. Specs can reference
//! other Specs to build composable logic.
//!
//! ### Data
//! Data are named values: numbers, text, dates, booleans, or typed units
//! like `50 kilograms` or `100`.
//!
//! ### Rules
//! Rules compute values based on data and other rules. They support
//! conditional logic through "unless" clauses.
//!
//! ### Types
//! Lemma has a rich type system including units (mass, length, time, money)
//! with automatic conversions.

#[cfg(test)]
mod tests;

pub(crate) mod computation;
pub mod deps;
pub mod engine;
pub mod error;
pub mod evaluation;
pub mod formatting;
pub mod inversion;
pub mod limits;
pub(crate) mod literals;
pub mod parsing;
pub mod planning;
pub mod registry;
pub mod serialization;
pub mod spec_set_id;
pub(crate) mod stdlib;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use computation::rational::{
    checked_mul, commit_rational_to_decimal, decimal_to_rational, rational_to_display_str,
    NumericFailure, NumericOperation, RationalInteger,
};
pub use deps::{
    dependency_cache_file, dependency_identifier_from_dependency_path, lemma_deps_dir,
    relative_dependency_cache_path,
};
#[cfg(not(target_arch = "wasm32"))]
pub use engine::collect_lemma_sources;
pub use engine::{Context, Engine, Errors, ResolvedRepository};
pub use error::{Error, ErrorKind, RequestErrorKind};
pub use evaluation::explanation;
pub use evaluation::operations::{OperationResult, VetoType};
pub use evaluation::request::{parse_rule_result_conversion_strings, EvaluationRequest};
pub use evaluation::response::{DataGroup, Response, RuleResult};
pub use formatting::format_source;
pub use inversion::{Bound, Domain, Target};
pub use limits::ResourceLimits;
pub use parsing::ast::{DateTimeValue, EffectiveDate, LemmaRepository, LemmaSpec};
pub use parsing::parse;
pub use parsing::source::SourceType;
pub use parsing::ParseResult;
pub use planning::semantics::{DataPath, LemmaType, LiteralValue, TypeSpecification, ValueKind};
pub use planning::{ExecutionPlan, LemmaSpecSet, SpecSchema};
