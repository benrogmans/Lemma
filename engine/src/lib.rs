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
pub mod result_value;
pub(crate) mod spec_set_id;
pub(crate) mod stdlib;

#[cfg(not(target_arch = "wasm32"))]
pub mod deps;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use computation::{OperationResult, VetoType};
pub use engine::{
    resolve_effective, Engine, Errors, ListedSpec, ResolvedRepository, EMBEDDED_STDLIB_REPOSITORY,
};
pub use error::{Error, ErrorDetails, ErrorKind, RequestErrorKind};
pub use evaluation::explanations::{format_explanation, Cause, Explanation, ExplanationNode};
pub use evaluation::response::{Response, RuleResult};
pub use evaluation::run_data::RunDataValue;
pub use formatting::{format_parse_result, format_source, format_specs};
pub use limits::{
    ResourceLimits, MAX_DATA_NAME_LENGTH, MAX_RULE_NAME_LENGTH, MAX_SPEC_NAME_LENGTH,
};
pub use literals::{DateGranularity, MeasureUnit, MeasureUnits, RatioUnit, RatioUnits};
pub use parsing::ast::DateTimeValue;
pub use parsing::source::SourceType;
pub use planning::execution_plan::type_detail_lines;
pub use planning::execution_plan::{Show, ShowData, ShowVersion};
pub use planning::explanation::{ConversionTraceRole, SerializedConversionTraceStep};
pub use planning::semantics::{DataPath, LemmaType, LiteralValue, TypeSpecification, ValueKind};
pub use result_value::{CalendarResult, RangeResult, RuleResultValue, RuleResultValueFailure};
pub use spec_set_id::parse_spec_set_id;
pub use stdlib::UNITS_LEMMA;

/// Exact rational helpers for in-tree integration tests. Not a supported consumer API.
#[doc(hidden)]
pub mod __test_support {
    pub use crate::computation::rational::{
        checked_div, checked_mul, decimal_to_rational, rational_new,
    };
    pub use crate::literals::TimeValue;
    pub use crate::planning::semantics::{SemanticDateTime, SemanticTime, SemanticTimezone};

    /// Serializes an [`Error`](crate::Error) the way WASM/`JsError` does today.
    /// API-contract tests assert the *target* shape against this until a single
    /// derived error type replaces the three hand-built projections.
    pub fn current_binding_error_json(error: &crate::Error) -> serde_json::Value {
        use crate::parsing::source::Source;
        use serde::Serialize;

        #[derive(Serialize)]
        struct JsSource {
            attribute: String,
            line: usize,
            column: usize,
            length: usize,
        }

        impl From<&Source> for JsSource {
            fn from(s: &Source) -> Self {
                Self {
                    attribute: s.source_type.to_string(),
                    line: s.span.line,
                    column: s.span.col,
                    length: s.span.end.saturating_sub(s.span.start),
                }
            }
        }

        #[derive(Serialize)]
        struct JsError<'a> {
            kind: crate::ErrorKind,
            message: &'a str,
            related_data: Option<&'a str>,
            spec: Option<&'a str>,
            related_spec: Option<&'a str>,
            source: Option<JsSource>,
            suggestion: Option<&'a str>,
            repository: Option<&'a str>,
            registry_kind: Option<crate::registry::RegistryErrorKind>,
            request_kind: Option<crate::error::RequestErrorKind>,
            limit_name: Option<&'a str>,
            limit_value: Option<&'a str>,
            actual_value: Option<&'a str>,
        }

        let shaped = JsError {
            kind: error.kind(),
            message: error.message(),
            related_data: error.related_data(),
            spec: error.spec_context_name(),
            related_spec: error.related_spec(),
            source: error.source_location().map(JsSource::from),
            suggestion: error.suggestion(),
            repository: error.repository(),
            registry_kind: error.registry_kind(),
            request_kind: error.request_kind(),
            limit_name: error.limit_name(),
            limit_value: error.limit_value(),
            actual_value: error.actual_value(),
        };
        serde_json::to_value(shaped).expect("JsError-shaped JSON must serialize")
    }
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
