use crate::parsing::ast::Span;
use crate::parsing::source::Source;
use crate::planning::semantics::{FactPath, RulePath};
use std::fmt;
use std::sync::Arc;

/// Detailed error information with source location
#[derive(Debug, Clone)]
pub struct ErrorDetails {
    pub message: String,
    pub source_location: Source,
    pub source_text: Arc<str>,
    pub doc_start_line: usize,
    pub suggestion: Option<String>,
}

/// Error types for the Lemma system with source location tracking
#[derive(Debug, Clone)]
pub enum LemmaError {
    /// Parse error with source location
    Parse(Box<ErrorDetails>),

    /// Semantic validation error with source location
    Semantic(Box<ErrorDetails>),

    /// Inversion error (valid Lemma, but unsupported by inversion) with source location
    Inversion(Box<ErrorDetails>),

    /// Runtime error during evaluation with source location
    Runtime(Box<ErrorDetails>),

    /// Engine error with source location
    Engine(Box<ErrorDetails>),

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
    MultipleErrors(Vec<LemmaError>),
}

impl LemmaError {
    /// Create a parse error with source information
    pub fn parse(
        message: impl Into<String>,
        span: Span,
        attribute: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Parse(Box::new(ErrorDetails {
            message: message.into(),
            source_location: Source::new(attribute, span, doc_name),
            source_text,
            doc_start_line,
            suggestion: suggestion.map(Into::into),
        }))
    }

    /// Create a parse error with suggestion
    pub fn parse_with_suggestion(
        message: impl Into<String>,
        span: Span,
        attribute: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::parse(
            message,
            span,
            attribute,
            source_text,
            doc_name,
            doc_start_line,
            Some(suggestion),
        )
    }

    /// Create a semantic error with source information
    pub fn semantic(
        message: impl Into<String>,
        span: Span,
        attribute: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Semantic(Box::new(ErrorDetails {
            message: message.into(),
            source_location: Source::new(attribute, span, doc_name),
            source_text,
            doc_start_line,
            suggestion: suggestion.map(Into::into),
        }))
    }

    /// Create a semantic error with suggestion
    pub fn semantic_with_suggestion(
        message: impl Into<String>,
        span: Span,
        attribute: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::semantic(
            message,
            span,
            attribute,
            source_text,
            doc_name,
            doc_start_line,
            Some(suggestion),
        )
    }

    /// Create an inversion error with source information
    pub fn inversion(
        message: impl Into<String>,
        source: &Source,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Inversion(Box::new(ErrorDetails {
            message: message.into(),
            source_location: source.clone(),
            source_text: Arc::from(""),
            doc_start_line: 1,
            suggestion: suggestion.map(Into::into),
        }))
    }

    /// Create an inversion error with suggestion
    pub fn inversion_with_suggestion(
        message: impl Into<String>,
        source: &Source,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::inversion(message, source, Some(suggestion))
    }

    /// Create an engine error with source information
    pub fn engine(
        message: impl Into<String>,
        span: Span,
        attribute: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Engine(Box::new(ErrorDetails {
            message: message.into(),
            source_location: Source::new(attribute, span, doc_name),
            source_text,
            doc_start_line,
            suggestion: suggestion.map(Into::into),
        }))
    }

    /// Create a missing fact error with source information
    pub fn missing_fact(
        fact_path: FactPath,
        span: Span,
        attribute: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::MissingFact(Box::new(ErrorDetails {
            message: format!("Missing fact: {}", fact_path),
            source_location: Source::new(attribute, span, doc_name),
            source_text,
            doc_start_line,
            suggestion: suggestion.map(Into::into),
        }))
    }

    /// Create a missing rule error with source information
    pub fn missing_rule(
        rule_path: RulePath,
        span: Span,
        attribute: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::Engine(Box::new(ErrorDetails {
            message: format!("Missing rule: {}", rule_path),
            source_location: Source::new(attribute, span, doc_name),
            source_text,
            doc_start_line,
            suggestion: suggestion.map(Into::into),
        }))
    }

    /// Create a circular dependency error with source information
    pub fn circular_dependency(
        message: impl Into<String>,
        source_location: Source,
        source_text: Arc<str>,
        doc_start_line: usize,
        cycle: Vec<Source>,
        suggestion: Option<impl Into<String>>,
    ) -> Self {
        Self::CircularDependency {
            details: Box::new(ErrorDetails {
                message: message.into(),
                source_location,
                source_text,
                doc_start_line,
                suggestion: suggestion.map(Into::into),
            }),
            cycle,
        }
    }
}

impl fmt::Display for LemmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LemmaError::Parse(details) => {
                write!(f, "Parse error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_location.attribute,
                    details.source_location.span.line,
                    details.source_location.span.col
                )
            }
            LemmaError::Semantic(details) => {
                write!(f, "Semantic error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_location.attribute,
                    details.source_location.span.line,
                    details.source_location.span.col
                )
            }
            LemmaError::Inversion(details) => {
                write!(f, "Inversion error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_location.attribute,
                    details.source_location.span.line,
                    details.source_location.span.col
                )
            }
            LemmaError::Runtime(details) => {
                write!(f, "Runtime error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_location.attribute,
                    details.source_location.span.line,
                    details.source_location.span.col
                )
            }
            LemmaError::Engine(details) => {
                write!(f, "Engine error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_location.attribute,
                    details.source_location.span.line,
                    details.source_location.span.col
                )
            }
            LemmaError::MissingFact(details) => {
                write!(f, "Missing fact: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_location.attribute,
                    details.source_location.span.line,
                    details.source_location.span.col
                )
            }
            LemmaError::CircularDependency { details, .. } => {
                write!(f, "Circular dependency: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_location.attribute,
                    details.source_location.span.line,
                    details.source_location.span.col
                )
            }
            LemmaError::ResourceLimitExceeded {
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
            LemmaError::MultipleErrors(errors) => {
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

impl std::error::Error for LemmaError {}

impl From<std::fmt::Error> for LemmaError {
    fn from(err: std::fmt::Error) -> Self {
        use crate::parsing::ast::Span;
        LemmaError::engine(
            format!("Format error: {err}"),
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<format-error>",
            Arc::from(""),
            "<format-error>",
            1,
            None::<String>,
        )
    }
}

impl LemmaError {
    /// Get the error message
    pub fn message(&self) -> &str {
        match self {
            LemmaError::Parse(details)
            | LemmaError::Semantic(details)
            | LemmaError::Inversion(details)
            | LemmaError::Runtime(details)
            | LemmaError::Engine(details)
            | LemmaError::MissingFact(details) => &details.message,
            LemmaError::CircularDependency { details, .. } => &details.message,
            LemmaError::ResourceLimitExceeded { limit_name, .. } => limit_name,
            LemmaError::MultipleErrors(_) => "Multiple errors occurred",
        }
    }

    /// Get the source location if available
    pub fn location(&self) -> Option<&Source> {
        match self {
            LemmaError::Parse(details)
            | LemmaError::Semantic(details)
            | LemmaError::Inversion(details)
            | LemmaError::Runtime(details)
            | LemmaError::Engine(details)
            | LemmaError::MissingFact(details) => Some(&details.source_location),
            LemmaError::CircularDependency { details, .. } => Some(&details.source_location),
            LemmaError::ResourceLimitExceeded { .. } | LemmaError::MultipleErrors(_) => None,
        }
    }

    /// Get the source text if available
    pub fn source_text(&self) -> Option<&str> {
        match self {
            LemmaError::Parse(details)
            | LemmaError::Semantic(details)
            | LemmaError::Inversion(details)
            | LemmaError::Runtime(details)
            | LemmaError::Engine(details)
            | LemmaError::MissingFact(details) => Some(&details.source_text),
            LemmaError::CircularDependency { details, .. } => Some(&details.source_text),
            LemmaError::ResourceLimitExceeded { .. } | LemmaError::MultipleErrors(_) => None,
        }
    }

    /// Get the suggestion if available
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            LemmaError::Parse(details)
            | LemmaError::Semantic(details)
            | LemmaError::Inversion(details)
            | LemmaError::Runtime(details)
            | LemmaError::Engine(details)
            | LemmaError::MissingFact(details) => details.suggestion.as_deref(),
            LemmaError::CircularDependency { details, .. } => details.suggestion.as_deref(),
            LemmaError::ResourceLimitExceeded { suggestion, .. } => Some(suggestion),
            LemmaError::MultipleErrors(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::Span;
    use std::sync::Arc;

    type ErrorVariant =
        fn(String, Span, String, Arc<str>, String, usize, Option<String>) -> LemmaError;

    #[allow(clippy::type_complexity)]
    fn create_test_error(variant: ErrorVariant) -> LemmaError {
        let source_text = "fact amount = 100";
        let span = Span {
            start: 14,
            end: 21,
            line: 1,
            col: 15,
        };
        variant(
            "Invalid currency".to_string(),
            span,
            "test.lemma".to_string(),
            Arc::from(source_text),
            "test_doc".to_string(),
            1,
            None,
        )
    }

    #[test]
    fn test_error_creation_and_display() {
        let parse_error = create_test_error(LemmaError::parse);
        let parse_error_display = format!("{parse_error}");
        assert!(parse_error_display.contains("Parse error: Invalid currency"));
        assert!(parse_error_display.contains("test.lemma:1:15"));

        let semantic_error = create_test_error(LemmaError::semantic);
        let semantic_error_display = format!("{semantic_error}");
        assert!(semantic_error_display.contains("Semantic error: Invalid currency"));
        assert!(semantic_error_display.contains("test.lemma:1:15"));

        let source_text = "fact amont = 100";
        let span = Span {
            start: 5,
            end: 10,
            line: 1,
            col: 6,
        };
        let parse_error_with_suggestion = LemmaError::parse_with_suggestion(
            "Typo in fact name",
            span.clone(),
            "suggestion.lemma",
            Arc::from(source_text),
            "suggestion_doc",
            1,
            "Did you mean 'amount'?",
        );
        let parse_error_with_suggestion_display = format!("{parse_error_with_suggestion}");
        assert!(parse_error_with_suggestion_display.contains("Typo in fact name"));
        assert!(parse_error_with_suggestion_display.contains("Did you mean 'amount'?"));

        let semantic_error_with_suggestion = LemmaError::semantic_with_suggestion(
            "Incompatible types",
            span.clone(),
            "suggestion.lemma",
            Arc::from(source_text),
            "suggestion_doc",
            1,
            "Try converting one of the types.",
        );
        let semantic_error_with_suggestion_display = format!("{semantic_error_with_suggestion}");
        assert!(semantic_error_with_suggestion_display.contains("Incompatible types"));
        assert!(semantic_error_with_suggestion_display.contains("Try converting one of the types."));

        let engine_error = LemmaError::engine(
            "Something went wrong",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<test>",
            Arc::from(""),
            "<test>",
            1,
            None::<String>,
        );
        assert!(format!("{engine_error}").contains("Engine error: Something went wrong"));

        let circular_dependency_error = LemmaError::circular_dependency(
            "a -> b -> a",
            Source::new(
                "<test>",
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<test>",
            ),
            Arc::from(""),
            1,
            vec![],
            None::<String>,
        );
        assert!(format!("{circular_dependency_error}").contains("Circular dependency: a -> b -> a"));

        let multiple_errors =
            LemmaError::MultipleErrors(vec![parse_error, semantic_error, engine_error]);
        let multiple_errors_display = format!("{multiple_errors}");
        assert!(multiple_errors_display.contains("Multiple errors:"));
        assert!(multiple_errors_display.contains("Parse error: Invalid currency"));
        assert!(multiple_errors_display.contains("Semantic error: Invalid currency"));
        assert!(multiple_errors_display.contains("Engine error: Something went wrong"));
    }
}
