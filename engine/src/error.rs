use crate::parsing::source::Source;
use crate::planning::semantics::FactPath;
use crate::registry::RegistryErrorKind;
use std::fmt;

/// Detailed error information with optional source location.
#[derive(Debug, Clone)]
pub struct ErrorDetails {
    pub message: String,
    pub source: Option<Source>,
    pub suggestion: Option<String>,
}

/// Error types for the Lemma system with source location tracking
#[derive(Debug, Clone)]
pub enum Error {
    /// Parse error with source location
    Parsing(Box<ErrorDetails>),

    /// Inversion error (valid Lemma, but unsupported by inversion) with source location
    Inversion(Box<ErrorDetails>),

    /// Planning error with source location
    Planning(Box<ErrorDetails>),

    /// Registry resolution error with source location and structured error kind.
    ///
    /// Produced when an `@...` reference cannot be resolved by the configured Registry
    /// (e.g. the document was not found, the request was unauthorized, or the network
    /// is unreachable).
    Registry {
        details: Box<ErrorDetails>,
        /// The `@...` identifier that failed to resolve (without the leading `@`).
        identifier: String,
        /// The category of failure.
        kind: RegistryErrorKind,
    },

    /// Missing fact error during evaluation with source location
    MissingFact(Box<ErrorDetails>),

    /// Circular dependency error with source location and cycle information
    CircularDependency {
        details: Box<ErrorDetails>,
        cycle: Vec<Source>,
    },

    /// Resource limit exceeded
    ResourceLimitExceeded {
        limit_name: String,
        limit_value: String,
        actual_value: String,
        suggestion: String,
    },

    /// Multiple errors collected together
    MultipleErrors(Vec<Error>),
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

    /// Create a planning error with source information
    pub fn planning(
        message: impl Into<String>,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Planning(Box::new(ErrorDetails {
            message: message.into(),
            source,
            suggestion: suggestion.map(Into::into),
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
            }),
            identifier: identifier.into(),
            kind,
        }
    }

    /// Create a missing fact error with source information
    pub fn missing_fact(
        fact_path: FactPath,
        source: Option<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::MissingFact(Box::new(ErrorDetails {
            message: format!("Missing fact: {}", fact_path),
            source,
            suggestion: suggestion.map(Into::into),
        }))
    }

    /// Create a circular dependency error with source information
    pub fn circular_dependency(
        message: impl Into<String>,
        source: Option<Source>,
        cycle: Vec<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::CircularDependency {
            details: Box::new(ErrorDetails {
                message: message.into(),
                source,
                suggestion: suggestion.map(Into::into),
            }),
            cycle,
        }
    }
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

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parsing(details) => {
                write!(f, "Parse error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_source_location(f, &details.source)
            }
            Error::Inversion(details) => {
                write!(f, "Inversion error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_source_location(f, &details.source)
            }
            Error::Planning(details) => {
                write!(f, "Planning error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_source_location(f, &details.source)
            }
            Error::Registry {
                details,
                identifier,
                kind,
            } => {
                write!(
                    f,
                    "Registry error ({}): @{}: {}",
                    kind, identifier, details.message
                )?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_source_location(f, &details.source)
            }
            Error::MissingFact(details) => {
                write!(f, "Missing fact: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_source_location(f, &details.source)
            }
            Error::CircularDependency { details, .. } => {
                write!(f, "Circular dependency: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write_source_location(f, &details.source)
            }
            Error::ResourceLimitExceeded {
                limit_name,
                limit_value,
                actual_value,
                suggestion,
            } => {
                write!(
                    f,
                    "Resource limit exceeded: {limit_name} (limit: {limit_value}, actual: {actual_value}). {suggestion}"
                )
            }
            Error::MultipleErrors(errors) => {
                writeln!(f, "Multiple errors:")?;
                for (i, error) in errors.iter().enumerate() {
                    write!(f, "  {}. {error}", i + 1)?;
                    if i < errors.len() - 1 {
                        writeln!(f)?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<std::fmt::Error> for Error {
    fn from(err: std::fmt::Error) -> Self {
        Error::planning(format!("Format error: {err}"), None, None::<String>)
    }
}

impl Error {
    /// Get the error message
    pub fn message(&self) -> &str {
        match self {
            Error::Parsing(details)
            | Error::Inversion(details)
            | Error::Planning(details)
            | Error::MissingFact(details) => &details.message,
            Error::Registry { details, .. } => &details.message,
            Error::CircularDependency { details, .. } => &details.message,
            Error::ResourceLimitExceeded { limit_name, .. } => limit_name,
            Error::MultipleErrors(_) => "Multiple errors occurred",
        }
    }

    /// Get the source location if available
    pub fn location(&self) -> Option<&Source> {
        match self {
            Error::Parsing(details)
            | Error::Inversion(details)
            | Error::Planning(details)
            | Error::MissingFact(details) => details.source.as_ref(),
            Error::Registry { details, .. } => details.source.as_ref(),
            Error::CircularDependency { details, .. } => details.source.as_ref(),
            Error::ResourceLimitExceeded { .. } | Error::MultipleErrors(_) => None,
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
            | Error::Planning(details)
            | Error::MissingFact(details) => details.suggestion.as_deref(),
            Error::Registry { details, .. } => details.suggestion.as_deref(),
            Error::CircularDependency { details, .. } => details.suggestion.as_deref(),
            Error::ResourceLimitExceeded { suggestion, .. } => Some(suggestion),
            Error::MultipleErrors(_) => None,
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
            "test_doc",
            Arc::from("fact amount = 100"),
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
            "suggestion_doc",
            Arc::from("fact amont = 100"),
        );

        let parse_error_with_suggestion = Error::parsing_with_suggestion(
            "Typo in fact name",
            Some(suggestion_source),
            "Did you mean 'amount'?",
        );
        let parse_error_with_suggestion_display = format!("{parse_error_with_suggestion}");
        assert!(parse_error_with_suggestion_display.contains("Typo in fact name"));
        assert!(parse_error_with_suggestion_display.contains("Did you mean 'amount'?"));

        let engine_error = Error::planning("Something went wrong", None, None::<String>);
        assert!(format!("{engine_error}").contains("Planning error: Something went wrong"));
        assert!(!format!("{engine_error}").contains(" at "));

        let circular_dependency_error =
            Error::circular_dependency("a -> b -> a", None, vec![], None::<String>);
        assert!(format!("{circular_dependency_error}").contains("Circular dependency: a -> b -> a"));

        let multiple_errors = Error::MultipleErrors(vec![parse_error, engine_error]);
        let multiple_errors_display = format!("{multiple_errors}");
        assert!(multiple_errors_display.contains("Multiple errors:"));
        assert!(multiple_errors_display.contains("Parse error: Invalid currency"));
        assert!(multiple_errors_display.contains("Planning error: Something went wrong"));
    }
}
