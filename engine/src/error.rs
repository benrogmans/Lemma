use crate::parsing::ast::LemmaSpec;
use crate::parsing::source::Source;
use crate::registry::RegistryErrorKind;
use std::fmt;
use std::sync::Arc;

/// Detailed error information with optional source location.
#[derive(Debug, Clone)]
pub struct ErrorDetails {
    pub message: String,
    pub source: Option<Source>,
    pub suggestion: Option<String>,
    /// Spec we were planning when this error occurred. Used for display grouping ("In spec 'X':").
    pub spec_context: Option<Arc<LemmaSpec>>,
    /// When the cause involves a referenced spec, that temporal version. Displayed as "See spec 'X' (active from Y)."
    pub related_spec: Option<Arc<LemmaSpec>>,
    /// Data name this error is about. Populated by the data-binding site so consumers can attribute
    /// the error to a specific input field without string parsing. Displayed as "Failed to parse data 'X':".
    pub related_data: Option<String>,
}

/// Classification of an [`Error`]. Serialized as the `kind` field on the flat object returned to JavaScript from WASM (`engine/src/wasm.rs`, `JsError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Parsing,
    Validation,
    Inversion,
    Registry,
    Request,
    ResourceLimit,
}

/// Error types for the Lemma system with source location tracking
#[derive(Debug, Clone)]
pub enum Error {
    /// Parse error with source location
    Parsing(Box<ErrorDetails>),

    /// Inversion error (valid Lemma, but unsupported by inversion) with source location
    Inversion(Box<ErrorDetails>),

    /// Validation error (semantic/planning, including circular dependency) with source location
    Validation(Box<ErrorDetails>),

    /// Registry resolution error with source location and structured error kind.
    ///
    /// Produced when an `@...` reference cannot be resolved by the configured Registry
    /// (e.g. the spec was not found, the request was unauthorized, or the network
    /// is unreachable).
    Registry {
        details: Box<ErrorDetails>,
        /// The `@...` identifier that failed to resolve (includes the leading `@`).
        identifier: String,
        /// The category of failure.
        kind: RegistryErrorKind,
    },

    /// Resource limit exceeded
    ResourceLimitExceeded {
        details: Box<ErrorDetails>,
        limit_name: String,
        limit_value: String,
        actual_value: String,
    },

    /// Request error: invalid or unsatisfiable API request (e.g. spec not found, invalid parameters).
    /// Not a parse/planning failure; the request itself is invalid. Such errors occur *before* any evaluation and *never during* evaluation.
    Request {
        details: Box<ErrorDetails>,
        kind: RequestErrorKind,
    },
}

/// Distinguishes HTTP 404 (not found) from 400 (bad request) for request errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestErrorKind {
    /// Spec not found or no temporal version for effective — map to 404.
    SpecNotFound,
    /// Rule not found
    RuleNotFound,
    /// Invalid spec id, etc. — map to 400.
    InvalidRequest,
}

impl Error {
    /// Create a parse error. Source is required: parsing errors always originate from source code.
    pub fn parsing(
        message: impl Into<String>,
        source: Source,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::parsing_with_context(message, source, suggestion, None, None)
    }

    /// Parse error with optional spec context (for display).
    pub fn parsing_with_context(
        message: impl Into<String>,
        source: Source,
        suggestion: Option<impl Into<String>>,
        spec_context: Option<Arc<LemmaSpec>>,
        related_spec: Option<Arc<LemmaSpec>>,
    ) -> Self {
        Self::Parsing(Box::new(ErrorDetails {
            message: message.into(),
            source: Some(source),
            suggestion: suggestion.map(Into::into),
            spec_context,
            related_spec,
            related_data: None,
        }))
    }

    /// Create a parse error with suggestion. Source is required.
    pub fn parsing_with_suggestion(
        message: impl Into<String>,
        source: Source,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::parsing_with_context(message, source, Some(suggestion), None, None)
    }

    /// Create an inversion error with source information.
    pub fn inversion(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::inversion_with_context(message, source, suggestion, None, None)
    }

    /// Inversion error with optional spec context (for display).
    pub fn inversion_with_context(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
        spec_context: Option<Arc<LemmaSpec>>,
        related_spec: Option<Arc<LemmaSpec>>,
    ) -> Self {
        Self::Inversion(Box::new(ErrorDetails {
            message: message.into(),
            source,
            suggestion: suggestion.map(Into::into),
            spec_context,
            related_spec,
            related_data: None,
        }))
    }

    /// Create an inversion error with suggestion
    pub fn inversion_with_suggestion(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: impl Into<String>,
        spec_context: Option<Arc<LemmaSpec>>,
        related_spec: Option<Arc<LemmaSpec>>,
    ) -> Self {
        Self::inversion_with_context(
            message,
            source,
            Some(suggestion),
            spec_context,
            related_spec,
        )
    }

    /// Create a validation error with source information (semantic/planning, including circular dependency).
    pub fn validation(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::validation_with_context(message, source, suggestion, None, None)
    }

    /// Validation error with optional spec context and related spec (for display).
    pub fn validation_with_context(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
        spec_context: Option<Arc<LemmaSpec>>,
        related_spec: Option<Arc<LemmaSpec>>,
    ) -> Self {
        Self::Validation(Box::new(ErrorDetails {
            message: message.into(),
            source,
            suggestion: suggestion.map(Into::into),
            spec_context,
            related_spec,
            related_data: None,
        }))
    }

    /// Create a request error (invalid API request, e.g. bad spec id).
    /// Request errors never have source locations — they are API-level.
    pub fn request(message: impl Into<String>, suggestion: Option<impl Into<String>>) -> Self {
        Self::request_with_kind(message, suggestion, RequestErrorKind::InvalidRequest)
    }

    /// Create a "spec not found" request error — map to HTTP 404.
    pub fn request_not_found(
        message: impl Into<String>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::request_with_kind(message, suggestion, RequestErrorKind::SpecNotFound)
    }

    /// Create a rule not found error
    pub fn rule_not_found(rule_name: &str, suggestion: Option<impl Into<String>>) -> Self {
        Self::request_with_kind(
            format!("Rule '{}' not found", rule_name),
            suggestion,
            RequestErrorKind::RuleNotFound,
        )
    }

    fn request_with_kind(
        message: impl Into<String>,
        suggestion: Option<impl Into<String>>,
        kind: RequestErrorKind,
    ) -> Self {
        Self::Request {
            details: Box::new(ErrorDetails {
                message: message.into(),
                source: None,
                suggestion: suggestion.map(Into::into),
                spec_context: None,
                related_spec: None,
                related_data: None,
            }),
            kind,
        }
    }

    /// Create a resource-limit-exceeded error with optional source location and spec context.
    pub fn resource_limit_exceeded(
        limit_name: impl Into<String>,
        limit_value: impl Into<String>,
        actual_value: impl Into<String>,
        suggestion: impl Into<String>,
        source: Option<Source>,
        spec_context: Option<Arc<LemmaSpec>>,
        related_spec: Option<Arc<LemmaSpec>>,
    ) -> Self {
        let limit_name = limit_name.into();
        let limit_value = limit_value.into();
        let actual_value = actual_value.into();
        let message = format!("{limit_name} (limit: {limit_value}, actual: {actual_value})");
        Self::ResourceLimitExceeded {
            details: Box::new(ErrorDetails {
                message,
                source,
                suggestion: Some(suggestion.into()),
                spec_context,
                related_spec,
                related_data: None,
            }),
            limit_name,
            limit_value,
            actual_value,
        }
    }

    /// Create a registry error. Source is required: registry errors point to `@ref` in source.
    pub fn registry(
        message: impl Into<String>,
        source: Source,
        identifier: impl Into<String>,
        kind: RegistryErrorKind,
        suggestion: Option<impl Into<String>>,
        spec_context: Option<Arc<LemmaSpec>>,
        related_spec: Option<Arc<LemmaSpec>>,
    ) -> Self {
        Self::Registry {
            details: Box::new(ErrorDetails {
                message: message.into(),
                source: Some(source),
                suggestion: suggestion.map(Into::into),
                spec_context,
                related_spec,
                related_data: None,
            }),
            identifier: identifier.into(),
            kind,
        }
    }

    /// Attach spec context for display grouping. Returns a new Error with context set.
    pub fn with_spec_context(self, spec: Arc<LemmaSpec>) -> Self {
        self.map_details(|d| d.spec_context = Some(spec))
    }

    /// Attach a data-binding attribution. Returns a new Error carrying the data name.
    /// Consumers (WASM `JsError`, LSP, HTTP) can read this via [`Error::related_data`] to attribute
    /// the failure to a specific input field without parsing strings.
    pub fn with_related_data(self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.map_details(|d| d.related_data = Some(name))
    }

    /// Apply a mutator to the inner [`ErrorDetails`] regardless of variant.
    fn map_details(self, f: impl FnOnce(&mut ErrorDetails)) -> Self {
        match self {
            Error::Parsing(details) => {
                let mut d = *details;
                f(&mut d);
                Error::Parsing(Box::new(d))
            }
            Error::Inversion(details) => {
                let mut d = *details;
                f(&mut d);
                Error::Inversion(Box::new(d))
            }
            Error::Validation(details) => {
                let mut d = *details;
                f(&mut d);
                Error::Validation(Box::new(d))
            }
            Error::Registry {
                details,
                identifier,
                kind,
            } => {
                let mut d = *details;
                f(&mut d);
                Error::Registry {
                    details: Box::new(d),
                    identifier,
                    kind,
                }
            }
            Error::ResourceLimitExceeded {
                details,
                limit_name,
                limit_value,
                actual_value,
            } => {
                let mut d = *details;
                f(&mut d);
                Error::ResourceLimitExceeded {
                    details: Box::new(d),
                    limit_name,
                    limit_value,
                    actual_value,
                }
            }
            Error::Request { details, kind } => {
                let mut d = *details;
                f(&mut d);
                Error::Request {
                    details: Box::new(d),
                    kind,
                }
            }
        }
    }
}

fn format_related_spec(spec: &LemmaSpec) -> String {
    let effective_from_str = spec
        .effective_from()
        .map(|d| d.to_string())
        .unwrap_or_else(|| "beginning".to_string());
    format!(
        "See spec '{}' (effective from {}).",
        spec.name, effective_from_str
    )
}

fn write_source_location(f: &mut fmt::Formatter<'_>, source: &Option<Source>) -> fmt::Result {
    if let Some(src) = source {
        write!(
            f,
            " at {}:{}:{}",
            src.attribute, src.span.line, src.span.col
        )
    } else {
        Ok(())
    }
}

fn write_related_spec(f: &mut fmt::Formatter<'_>, details: &ErrorDetails) -> fmt::Result {
    if let Some(ref related) = details.related_spec {
        write!(f, " {}", format_related_spec(related))?;
    }
    Ok(())
}

fn write_spec_context(f: &mut fmt::Formatter<'_>, spec: &LemmaSpec) -> fmt::Result {
    write!(f, "In spec '{}': ", spec.name)
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parsing(details) => {
                if let Some(ref spec) = details.spec_context {
                    write_spec_context(f, spec)?;
                }
                write!(f, "Parse error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_related_spec(f, details)?;
                write_source_location(f, &details.source)
            }
            Error::Inversion(details) => {
                if let Some(ref spec) = details.spec_context {
                    write_spec_context(f, spec)?;
                }
                write!(f, "Inversion error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_related_spec(f, details)?;
                write_source_location(f, &details.source)
            }
            Error::Validation(details) => {
                if let Some(ref spec) = details.spec_context {
                    write_spec_context(f, spec)?;
                }
                write!(f, "Validation error: ")?;
                if let Some(ref name) = details.related_data {
                    write!(f, "Failed to parse data '{}': ", name)?;
                }
                write!(f, "{}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_related_spec(f, details)?;
                write_source_location(f, &details.source)
            }
            Error::Registry {
                details,
                identifier,
                kind,
            } => {
                if let Some(ref spec) = details.spec_context {
                    write_spec_context(f, spec)?;
                }
                write!(
                    f,
                    "Registry error ({}): {}: {}",
                    kind, identifier, details.message
                )?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_related_spec(f, details)?;
                write_source_location(f, &details.source)
            }
            Error::ResourceLimitExceeded {
                details,
                limit_name,
                limit_value,
                actual_value,
            } => {
                if let Some(ref spec) = details.spec_context {
                    write_spec_context(f, spec)?;
                }
                write!(
                    f,
                    "Resource limit exceeded: {limit_name} (limit: {limit_value}, actual: {actual_value})"
                )?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, ". {suggestion}")?;
                }
                write_source_location(f, &details.source)
            }
            Error::Request { details, .. } => {
                if let Some(ref spec) = details.spec_context {
                    write_spec_context(f, spec)?;
                }
                write!(f, "Request error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_related_spec(f, details)?;
                write_source_location(f, &details.source)
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<std::fmt::Error> for Error {
    fn from(err: std::fmt::Error) -> Self {
        Error::validation(format!("Format error: {err}"), None, None::<String>)
    }
}

impl Error {
    /// Classify this error. Used by FFI/WASM consumers that need to branch on error category
    /// without depending on internal variant shapes.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::Parsing(_) => ErrorKind::Parsing,
            Error::Validation(_) => ErrorKind::Validation,
            Error::Inversion(_) => ErrorKind::Inversion,
            Error::Registry { .. } => ErrorKind::Registry,
            Error::Request { .. } => ErrorKind::Request,
            Error::ResourceLimitExceeded { .. } => ErrorKind::ResourceLimit,
        }
    }

    /// Shared access to the inner [`ErrorDetails`] regardless of variant.
    fn details(&self) -> &ErrorDetails {
        match self {
            Error::Parsing(d) | Error::Inversion(d) | Error::Validation(d) => d,
            Error::Registry { details, .. }
            | Error::ResourceLimitExceeded { details, .. }
            | Error::Request { details, .. } => details,
        }
    }

    /// Get the error message.
    pub fn message(&self) -> &str {
        &self.details().message
    }

    /// Get the source location if available.
    pub fn location(&self) -> Option<&Source> {
        self.details().source.as_ref()
    }

    /// Alias for [`Error::location`]. Preferred name when building the WASM/JS error payload.
    pub fn source_location(&self) -> Option<&Source> {
        self.location()
    }

    /// Resolve source text from the sources map (for display). Source no longer stores text.
    pub fn source_text(
        &self,
        sources: &std::collections::HashMap<String, String>,
    ) -> Option<String> {
        self.location()
            .and_then(|s| s.text_from(sources).map(|c| c.into_owned()))
    }

    /// Get the suggestion if available.
    pub fn suggestion(&self) -> Option<&str> {
        self.details().suggestion.as_deref()
    }

    /// Data name this error is attributed to (set at the data-binding call site).
    pub fn related_data(&self) -> Option<&str> {
        self.details().related_data.as_deref()
    }

    /// Name of the spec being planned when the error occurred.
    pub fn spec(&self) -> Option<&str> {
        self.details()
            .spec_context
            .as_ref()
            .map(|s| s.name.as_str())
    }

    /// Name of a related spec referenced by this error (e.g. a transitive dependency).
    pub fn related_spec(&self) -> Option<&str> {
        self.details()
            .related_spec
            .as_ref()
            .map(|s| s.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::Span;

    fn test_source() -> Source {
        Source::new(
            "test.lemma",
            Span {
                start: 14,
                end: 21,
                line: 1,
                col: 15,
            },
        )
    }

    #[test]
    fn test_error_creation_and_display() {
        let parse_error = Error::parsing("Invalid currency", test_source(), None::<String>);
        let parse_error_display = format!("{parse_error}");
        assert!(parse_error_display.contains("Parse error: Invalid currency"));
        assert!(parse_error_display.contains("test.lemma:1:15"));

        let suggestion_source = Source::new(
            "suggestion.lemma",
            Span {
                start: 5,
                end: 10,
                line: 1,
                col: 6,
            },
        );

        let parse_error_with_suggestion = Error::parsing_with_suggestion(
            "Typo in data name",
            suggestion_source,
            "Did you mean 'amount'?",
        );
        let parse_error_with_suggestion_display = format!("{parse_error_with_suggestion}");
        assert!(parse_error_with_suggestion_display.contains("Typo in data name"));
        assert!(parse_error_with_suggestion_display.contains("Did you mean 'amount'?"));

        let engine_error = Error::validation("Something went wrong", None, None::<String>);
        assert!(format!("{engine_error}").contains("Validation error: Something went wrong"));
        assert!(!format!("{engine_error}").contains(" at "));

        let validation_error =
            Error::validation("Circular dependency: a -> b -> a", None, None::<String>);
        assert!(format!("{validation_error}")
            .contains("Validation error: Circular dependency: a -> b -> a"));
    }

    #[test]
    fn test_error_kind_accessor() {
        assert_eq!(
            Error::parsing("x", test_source(), None::<String>).kind(),
            ErrorKind::Parsing
        );
        assert_eq!(
            Error::validation("x", None, None::<String>).kind(),
            ErrorKind::Validation
        );
        assert_eq!(
            Error::inversion("x", None, None::<String>).kind(),
            ErrorKind::Inversion
        );
        assert_eq!(
            Error::request("x", None::<String>).kind(),
            ErrorKind::Request
        );
        assert_eq!(
            Error::resource_limit_exceeded("cap", "1", "2", "try less", None, None, None).kind(),
            ErrorKind::ResourceLimit
        );
    }

    #[test]
    fn test_related_data_attribution_and_display() {
        let err = Error::validation(
            "Unknown unit 'mete' for this scale type",
            Some(test_source()),
            None::<String>,
        )
        .with_related_data("bridge_height");

        assert_eq!(err.related_data(), Some("bridge_height"));
        assert_eq!(err.kind(), ErrorKind::Validation);
        assert_eq!(err.message(), "Unknown unit 'mete' for this scale type");

        let display = format!("{err}");
        assert!(
            display.contains(
                "Validation error: Failed to parse data 'bridge_height': Unknown unit 'mete'"
            ),
            "unexpected display: {display}"
        );

        let at_occurrences = display.matches(" at ").count();
        assert_eq!(
            at_occurrences, 1,
            "expected exactly one ` at ` in display, got {at_occurrences}: {display}"
        );
    }

    #[test]
    fn test_related_data_none_by_default() {
        let err = Error::validation("x", None, None::<String>);
        assert!(err.related_data().is_none());
        assert!(err.spec().is_none());
        assert!(err.related_spec().is_none());
    }

    #[test]
    fn test_related_data_builder_preserves_other_variants() {
        let err = Error::resource_limit_exceeded(
            "max_data_value_bytes",
            "100",
            "200",
            "reduce size",
            Some(test_source()),
            None,
            None,
        )
        .with_related_data("big_blob");

        assert_eq!(err.kind(), ErrorKind::ResourceLimit);
        assert_eq!(err.related_data(), Some("big_blob"));
    }
}
