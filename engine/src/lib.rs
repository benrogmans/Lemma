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
//!     doc example
//!     fact price = 100
//!     fact quantity = 5
//!     rule total = price * quantity
//! "#.to_string());
//!
//! tokio::runtime::Runtime::new().unwrap()
//!     .block_on(engine.add_lemma_files(files))
//!     .expect("failed to add files");
//!
//! // Evaluate the document (all rules, no fact values)
//! let response = engine.evaluate("example", vec![], HashMap::new()).unwrap();
//! ```
//!
//! ## Core Concepts
//!
//! ### Documents
//! A document is a collection of facts and rules. Documents can reference
//! other documents to build composable logic.
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

pub mod computation;
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

pub use engine::Engine;
pub use error::Error;
pub use evaluation::operations::{
    ComputationKind, OperationKind, OperationRecord, OperationResult,
};
pub use evaluation::proof;
pub use evaluation::response::{Facts, Response, RuleResult};
pub use formatting::{format_docs, format_source};
pub use inversion::{
    invert, Bound, DerivedExpression, Domain, InversionResponse, Solution, Target, TargetOp,
};
pub use limits::ResourceLimits;
pub use parsing::ast::*;
pub use parsing::ast::{DepthTracker, Span};
pub use parsing::parse;
pub use parsing::Source;
pub use planning::semantics::{
    FactPath, LemmaType, LiteralValue, RatioUnit, RatioUnits, RulePath, ScaleUnit, ScaleUnits,
    SemanticDurationUnit, TypeSpecification, ValueKind,
};
pub use planning::{DocumentSchema, ExecutionPlan};
#[cfg(feature = "registry")]
pub use registry::LemmaBase;
pub use registry::{
    resolve_registry_references, Registry, RegistryBundle, RegistryError, RegistryErrorKind,
};

/// Result type for Lemma operations
pub type LemmaResult<T> = Result<T, Error>;
