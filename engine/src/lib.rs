//! # Lemma Engine
//!
//! **Rules for man and machine**
//!
//! Lemma is a declarative programming language for expressing rules, facts, and business logic
//! in a way that is both human-readable and machine-executable.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use lemma::Engine;
//! use std::collections::HashMap;
//!
//! let mut engine = Engine::new();
//!
//! // Load Lemma code
//! let mut files = HashMap::new();
//! files.insert("example.lemma".to_string(), r#"
//!     spec example
//!     fact price: 100
//!     fact quantity: 5
//!     rule total: price * quantity
//! "#.to_string());
//!
//! engine.add_lemma_files(files).expect("failed to add files");
//!
//! // Evaluate the spec (all rules, no fact values)
//! let now = lemma::DateTimeValue::now();
//! let response = engine.evaluate("example", None, &now, vec![], HashMap::new()).unwrap();
//! ```
//!
//! ## Core Concepts
//!
//! ### Specs
//! A spec is a collection of facts and rules. Specs can reference
//! other specs to build composable logic.
//!
//! ### Facts
//! Facts are named values: numbers, text, dates, booleans, or typed units
//! like `50 kilograms` or `100`.
//!
//! ### Rules
//! Rules compute values based on facts and other rules. They support
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
pub mod parsing;
pub mod planning;
pub mod registry;
pub mod serialization;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use engine::{Context, Engine};
pub use error::Error;
pub use evaluation::operations::{
    ComputationKind, OperationKind, OperationRecord, OperationResult,
};
pub use evaluation::proof;
pub use evaluation::response::{Facts, Response, RuleResult};
pub use formatting::{format_source, format_specs};
pub use inversion::{Bound, Domain, InversionResponse, Solution, Target, TargetOp};
pub use limits::ResourceLimits;
pub use parsing::ast::{
    DateTimeValue, DepthTracker, LemmaFact, LemmaRule, LemmaSpec, MetaField, MetaValue, Span,
    TypeDef,
};
pub use parsing::parse;
pub use parsing::ParseResult;
pub use parsing::Source;
pub use planning::semantics::{
    FactPath, LemmaType, LiteralValue, RatioUnit, RatioUnits, RulePath, ScaleUnit, ScaleUnits,
    SemanticDurationUnit, TypeSpecification, ValueKind,
};
pub use planning::{ExecutionPlan, PlanningResult, SpecPlanningResult, SpecSchema};
#[cfg(feature = "registry")]
pub use registry::LemmaBase;
pub use registry::{
    resolve_registry_references, Registry, RegistryBundle, RegistryError, RegistryErrorKind,
};
