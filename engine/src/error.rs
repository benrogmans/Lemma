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
    /// When the cause involves a referenced spec, that temporal version. Displayed as "See spec 'X' (active from Y)."
    pub related_spec: Option<Arc<LemmaSpec>>,
    /// Spec we were planning when this error occurred. Used for display grouping ("In spec 'X':").
    pub spec_context: Option<Arc<LemmaSpec>>,
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
    Request(Box<ErrorDetails>),
}

impl Error {
    /// Create a parse error with source information
    pub fn parsing(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Parsing(Box::new(ErrorDetails {
            message: message.into(),
            source,
            suggestion: suggestion.map(Into::into),
            related_spec: None,
            spec_context: None,
        }))
    }

    /// Create a parse error with suggestion
    pub fn parsing_with_suggestion(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::parsing(message, source, Some(suggestion))
    }

    /// Create an inversion error with source information
    pub fn inversion(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Inversion(Box::new(ErrorDetails {
            message: message.into(),
            source,
            suggestion: suggestion.map(Into::into),
            related_spec: None,
            spec_context: None,
        }))
    }

    /// Create an inversion error with suggestion
    pub fn inversion_with_suggestion(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::inversion(message, source, Some(suggestion))
    }

    /// Create a validation error with source information (semantic/planning, including circular dependency).
    pub fn validation(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Validation(Box::new(ErrorDetails {
            message: message.into(),
            source,
            suggestion: suggestion.map(Into::into),
            related_spec: None,
            spec_context: None,
        }))
    }

    /// Create a request error (invalid API request, e.g. spec not found).
    pub fn request(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Request(Box::new(ErrorDetails {
            message: message.into(),
            source,
            suggestion: suggestion.map(Into::into),
            related_spec: None,
            spec_context: None,
        }))
    }

    /// Create a resource-limit-exceeded error with optional source location.
    pub fn resource_limit_exceeded(
        limit_name: impl Into<String>,
        limit_value: impl Into<String>,
        actual_value: impl Into<String>,
        suggestion: impl Into<String>,
        source: Option<Source>,
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
                related_spec: None,
                spec_context: None,
            }),
            limit_name,
            limit_value,
            actual_value,
        }
    }

    /// Create a validation error with optional related spec (for spec-interface errors).
    /// When related_spec is set, Display shows "See spec 'X' (active from Y)."
    pub fn validation_with_context(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
        related_spec: Option<Arc<LemmaSpec>>,
    ) -> Self {
        Self::Validation(Box::new(ErrorDetails {
            message: message.into(),
            source,
            suggestion: suggestion.map(Into::into),
            related_spec,
            spec_context: None,
        }))
    }

    /// Create a registry error with source information and structured error kind.
    pub fn registry(
        message: impl Into<String>,
        source: Option<Source>,
        identifier: impl Into<String>,
        kind: RegistryErrorKind,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Registry {
            details: Box::new(ErrorDetails {
                message: message.into(),
                source,
                suggestion: suggestion.map(Into::into),
                related_spec: None,
                spec_context: None,
            }),
            identifier: identifier.into(),
            kind,
        }
    }

    /// Attach spec context for display grouping. Returns a new Error with context set.
    pub fn with_spec_context(self, spec: Arc<LemmaSpec>) -> Self {
        match self {
            Error::Parsing(details) => {
                let mut d = *details;
                d.spec_context = Some(spec.clone());
                Error::Parsing(Box::new(d))
            }
            Error::Inversion(details) => {
                let mut d = *details;
                d.spec_context = Some(spec.clone());
                Error::Inversion(Box::new(d))
            }
            Error::Validation(details) => {
                let mut d = *details;
                d.spec_context = Some(spec.clone());
                Error::Validation(Box::new(d))
            }
            Error::Registry {
                details,
                identifier,
                kind,
            } => {
                let mut d = *details;
                d.spec_context = Some(spec.clone());
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
                d.spec_context = Some(spec.clone());
                Error::ResourceLimitExceeded {
                    details: Box::new(d),
                    limit_name,
                    limit_value,
                    actual_value,
                }
            }
            Error::Request(details) => {
                let mut d = *details;
                d.spec_context = Some(spec);
                Error::Request(Box::new(d))
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
                write!(f, "Validation error: {}", details.message)?;
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
            Error::Request(details) => {
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
    /// Get the error message.
    pub fn message(&self) -> &str {
        match self {
            Error::Parsing(details)
            | Error::Inversion(details)
            | Error::Validation(details)
            | Error::Request(details) => &details.message,
            Error::Registry { details, .. } | Error::ResourceLimitExceeded { details, .. } => {
                &details.message
            }
        }
    }

    /// Get the source location if available
    pub fn location(&self) -> Option<&Source> {
        match self {
            Error::Parsing(details)
            | Error::Inversion(details)
            | Error::Validation(details)
            | Error::Request(details) => details.source.as_ref(),
            Error::Registry { details, .. } | Error::ResourceLimitExceeded { details, .. } => {
                details.source.as_ref()
            }
        }
    }

    /// Get the source text if available
    pub fn source_text(&self) -> Option<&str> {
        self.location().map(|s| &*s.source_text)
    }

    /// Get the suggestion if available
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            Error::Parsing(details)
            | Error::Inversion(details)
            | Error::Validation(details)
            | Error::Request(details) => details.suggestion.as_deref(),
            Error::Registry { details, .. } | Error::ResourceLimitExceeded { details, .. } => {
                details.suggestion.as_deref()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::Span;
    use std::sync::Arc;

    fn test_source() -> Source {
        Source::new(
            "test.lemma",
            Span {
                start: 14,
                end: 21,
                line: 1,
                col: 15,
            },
            "test_spec",
            Arc::from("fact amount: 100"),
        )
    }

    #[test]
    fn test_error_creation_and_display() {
        let parse_error = Error::parsing("Invalid currency", Some(test_source()), None::<String>);
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
            "suggestion_spec",
            Arc::from("fact amont: 100"),
        );

        let parse_error_with_suggestion = Error::parsing_with_suggestion(
            "Typo in fact name",
            Some(suggestion_source),
            "Did you mean 'amount'?",
        );
        let parse_error_with_suggestion_display = format!("{parse_error_with_suggestion}");
        assert!(parse_error_with_suggestion_display.contains("Typo in fact name"));
        assert!(parse_error_with_suggestion_display.contains("Did you mean 'amount'?"));

        let engine_error = Error::validation("Something went wrong", None, None::<String>);
        assert!(format!("{engine_error}").contains("Validation error: Something went wrong"));
        assert!(!format!("{engine_error}").contains(" at "));

        let validation_error =
            Error::validation("Circular dependency: a -> b -> a", None, None::<String>);
        assert!(format!("{validation_error}")
            .contains("Validation error: Circular dependency: a -> b -> a"));
    }
}
