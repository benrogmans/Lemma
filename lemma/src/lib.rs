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
//! use lemma::{Engine, LemmaResult};
//!
//! fn main() -> LemmaResult<()> {
//!     let mut engine = Engine::new();
//!
//!     // Load Lemma code
//!     engine.add_lemma_code(r#"
//!         doc example
//!         fact price = 100 USD
//!         fact quantity = 5
//!         rule total = price * quantity
//!     "#, "example.lemma")?;
//!
//!     // Evaluate the document
//!     let response = engine.evaluate("example", None, None)?;
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
//! like `50 kilograms` or `100 USD`.
//!
//! ### Rules
//! Rules compute values based on facts and other rules. They support
//! conditional logic through "unless" clauses.
//!
//! ### Types
//! Lemma has a rich type system including units (mass, length, time, money)
//! with automatic conversions.

pub mod analysis;
pub mod ast;
pub mod engine;
pub mod error;
pub mod evaluator;
pub mod inversion;
pub mod operation_result;
pub mod parser;
pub mod resource_limits;
pub mod response;
pub mod semantic;
pub mod serializers;
pub mod validator;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use ast::{ExpressionId, ExpressionIdGenerator, Span};
pub use engine::Engine;
/// Temporary alias to align with the Inversion plan's unified naming.
/// Workspace is functionally identical to Engine and will eventually replace it.
pub type Workspace = Engine;
pub use error::LemmaError;
pub use inversion::{Bound, BranchOutcome, Domain, Shape, ShapeBranch, Target, TargetOp};
pub use operation_result::OperationResult;
pub use parser::{parse, parse_facts};
pub use resource_limits::ResourceLimits;
pub use response::{OperationRecord, Response, RuleResult};
pub use semantic::*;
pub use validator::{ValidatedDocuments, Validator};

/// Result type for Lemma operations
pub type LemmaResult<T> = Result<T, LemmaError>;

#[cfg(test)]
mod tests;
