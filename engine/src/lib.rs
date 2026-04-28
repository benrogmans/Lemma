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
//! use std::collections::HashMap;
//!
//! let mut engine = Engine::new();
//!
//! // Load Lemma code
//! engine.load(r#"
//!     spec example
//!     data price: 100
//!     data quantity: 5
//!     rule total: price * quantity
//! "#, SourceType::Labeled("example.lemma")).expect("failed to load");
//!
//! // Evaluate the spec (all rules, no data values)
//! let now = lemma::DateTimeValue::now();
//! let response = engine.run("example", Some(&now), HashMap::new(), false).unwrap();
//! ```
//!
//! ## Core Concepts
//!
//! ### Specs
//! A spec is a collection of data and rules. Specs can reference
//! other specs to build composable logic.
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

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use engine::{Context, Engine, Errors, SourceType};
pub use error::{Error, ErrorKind, RequestErrorKind};
pub use evaluation::explanation;
pub use evaluation::operations::{
    ComputationKind, OperationKind, OperationRecord, OperationResult, VetoType,
};
pub use evaluation::response::{DataGroup, Response, RuleResult};
pub use formatting::{format_source, format_specs};
pub use inversion::{Bound, Domain, InversionResponse, Solution, Target, TargetOp};
pub use limits::ResourceLimits;
pub use parsing::ast::{
    DateTimeValue, DepthTracker, LemmaData, LemmaRule, LemmaSpec, MetaField, MetaValue, Span,
};
pub use parsing::parse;
pub use parsing::ParseResult;
pub use parsing::Source;
pub use planning::semantics::{
    is_same_spec, DataPath, LemmaType, LiteralValue, RatioUnit, RatioUnits, RulePath, ScaleUnit,
    ScaleUnits, SemanticDurationUnit, TypeDefiningSpec, TypeSpecification, ValueKind,
};
pub use planning::{
    ExecutionPlan, ExecutionPlanSet, LemmaSpecSet, PlanningResult, SpecPlanningResult, SpecSchema,
    SpecSetPlanningResult,
};
#[cfg(feature = "registry")]
pub use registry::LemmaBase;
pub use registry::{
    resolve_registry_references, Registry, RegistryBundle, RegistryError, RegistryErrorKind,
};
pub use spec_set_id::parse_spec_set_id;
