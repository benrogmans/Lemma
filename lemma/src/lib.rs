//! # Lemma Engine
//!
//! **Rules for man and machine**
//!
//! Lemma is a declarative programming language for expressing rules, facts, and business logic
//! in a way that is both human-readable and machine-executable.
//!
//! ## Quick Start
//!
//! ```rust
//! use lemma::{Engine, LemmaResult};
//!
//! fn main() -> LemmaResult<()> {
//!     let mut engine = Engine::new();
//!
//!     // Load Lemma code
//!     engine.add_lemma_code(r#"
//!         doc example
//!         fact price = 100
//!         fact quantity = 5
//!         rule total = price * quantity
//!     "#, "example.lemma")?;
//!
//!     // Evaluate the document (all rules, no fact values)
//!     let response = engine.evaluate("example", vec![], std::collections::HashMap::new())?;
//!
//!     Ok(())
//! }
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

pub mod computation;
pub mod engine;
pub mod error;
pub mod evaluation;
pub mod inversion;
pub mod limits;
pub mod parsing;
pub mod planning;
pub mod serialization;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use engine::Engine;
pub use error::LemmaError;
pub use evaluation::operations::{
    ComputationKind, OperationKind, OperationRecord, OperationResult,
};
pub use evaluation::proof;
pub use evaluation::response::{Facts, Response, RuleResult};
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
pub use planning::ExecutionPlan;

/// Result type for Lemma operations
pub type LemmaResult<T> = Result<T, LemmaError>;
