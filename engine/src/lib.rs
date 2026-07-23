//! # Lemma Engine
//!
//! **Rules for man and machine**
//!
//! Consumer API: **`load`** → **`list`** → **`show`** / **`source`** → **`run`**.

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

pub use engine::{
    resolve_effective, Engine, Errors, ListedSpec, ResolvedRepository, EMBEDDED_STDLIB_REPOSITORY,
};
pub use error::{Error, ErrorDetails, ErrorKind, RequestErrorKind};
pub use evaluation::data_input::DataValueInput;
pub use evaluation::explanations::{format_explanation, Cause, Explanation, ExplanationNode};
pub use evaluation::operations::{OperationResult, VetoType};
pub use evaluation::response::{Response, RuleResult};
pub use formatting::{format_parse_result, format_source, format_specs};
pub use limits::{
    ResourceLimits, MAX_DATA_NAME_LENGTH, MAX_RULE_NAME_LENGTH, MAX_SPEC_NAME_LENGTH,
};
pub use literals::{DateGranularity, MeasureUnit, MeasureUnits, RatioUnit, RatioUnits};
pub use parsing::ast::DateTimeValue;
pub use parsing::source::SourceType;
pub use planning::execution_plan::type_detail_lines;
pub use planning::execution_plan::{DataEntry, Show, ShowVersion};
pub use planning::explanation::{ConversionTraceRole, SerializedConversionTraceStep};
pub use planning::semantics::{
    DataPath, DataValue as BindingDataValue, LemmaType, LiteralValue, TypeSpecification, ValueKind,
};
pub use spec_set_id::parse_spec_set_id;
pub use stdlib::UNITS_LEMMA;

/// Exact rational helpers for in-tree integration tests. Not a supported consumer API.
#[doc(hidden)]
pub mod __test_support {
    pub use crate::computation::rational::{
        checked_div, checked_mul, decimal_to_rational, rational_new,
    };
}

// Tier 1 — language surface (always)
pub use parsing::ast::{
    try_parse_type_constraint_command, DataValue, Span, SpecRef, TimezoneValue,
};
pub use parsing::lexer::{Lexer, TokenKind};
pub use parsing::source::Source;
pub use parsing::{parse, ParseResult};

// Tier 2 — registry network (feature-gated)
#[cfg(feature = "registry")]
pub use engine::Context;
#[cfg(feature = "registry")]
pub use parsing::ast::{LemmaRepository, LemmaSpec};
#[cfg(feature = "registry")]
pub use planning::LemmaSpecSet;
#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
pub use registry::resolve_registry_references;
#[cfg(feature = "registry")]
pub use registry::{LemmaBase, Registry, RegistryBundle, RegistryError, RegistryErrorKind};
