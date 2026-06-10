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
//! let response = engine.run(None, "example", Some(&now), std::collections::HashMap::new(), false, None).unwrap();
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
pub(crate) mod engine;
pub(crate) mod error;
pub(crate) mod evaluation;
pub(crate) mod formatting;
pub(crate) mod limits;
pub(crate) mod literals;
pub(crate) mod parsing;
pub(crate) mod planning;
pub(crate) mod registry;
pub(crate) mod spec_set_id;
pub(crate) mod stdlib;

#[cfg(not(target_arch = "wasm32"))]
pub mod deps;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

// === Construction + orchestration ===
#[cfg(not(target_arch = "wasm32"))]
pub use engine::{
    collect_lemma_sources, Context, Engine, Errors, ResolvedRepository, EMBEDDED_STDLIB_REPOSITORY,
};
#[cfg(target_arch = "wasm32")]
pub use engine::{Context, Engine, Errors, ResolvedRepository, EMBEDDED_STDLIB_REPOSITORY};

// === Errors ===
pub use error::{Error, ErrorDetails, ErrorKind, RequestErrorKind};

// === Limits ===
pub use limits::{
    ResourceLimits, MAX_DATA_NAME_LENGTH, MAX_RULE_NAME_LENGTH, MAX_SPEC_NAME_LENGTH,
};

// === Source + parsing ===
pub use parsing::ast::{
    DataValue, DateGranularity, DateTimeValue, EffectiveDate, LemmaRepository, LemmaSpec, Span,
    SpecRef, TimezoneValue,
};
pub use parsing::source::{Source, SourceType};
// Planning [`semantics::DataValue`] (bindings) vs parse [`ast::DataValue`]; only the parse enum is `DataValue` at the root.
pub use parsing::lexer::{Lexer, TokenKind};
pub use parsing::{parse, ParseResult};
pub use planning::semantics::DataValue as BindingDataValue;

// === Data input ===
pub use planning::data_input::DataValueInput;

// === Execution plan + schema ===
pub use planning::execution_plan::{
    type_detail_lines, validate_instruction_jumps, validate_instructions, DataEntry, DataOverlay,
    ExecutionPlan, ExecutionPlanSerialized, Instruction, Instructions, SpecSchema,
    INSTRUCTIONS_VERSION,
};
pub use planning::plan;
pub use planning::semantics::{
    DataDefinition, DataPath, LemmaType, LiteralValue, QuantityUnit, QuantityUnits, RatioUnit,
    RatioUnits, RulePath, Source as PlanningSource, TypeSpecification, ValueKind,
};
pub use planning::spec_set::LemmaSpecSet;

// === Evaluation output ===
pub use evaluation::explanations::{
    format_explanation, format_expression, Cause, ConversionTraceRole, ConversionTraceStep,
    Explanation, ExplanationNode,
};
pub use evaluation::operations::{ComputationKind, OperationResult, VetoType};
pub use evaluation::response::{DataGroup, Response, RuleResult};

// === Formatting ===
pub use formatting::{format_parse_result, format_source, format_specs};

// === Registry ===
#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
pub use registry::resolve_registry_references;
pub use registry::{LemmaBase, Registry, RegistryBundle, RegistryError, RegistryErrorKind};

// === Spec set ID ===
pub use spec_set_id::parse_spec_set_id;

// === Stdlib ===
pub use stdlib::UNITS_LEMMA;
