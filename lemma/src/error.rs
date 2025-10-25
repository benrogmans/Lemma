use crate::ast::Span;
use std::fmt;
use std::sync::Arc;

/// Detailed error information with source location
#[derive(Debug, Clone)]
pub struct ErrorDetails {
    pub message: String,
    pub span: Span,
    pub source_id: String,
    pub source_text: Arc<str>,
    pub doc_name: String,
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

    /// Runtime error during evaluation with source location
    Runtime(Box<ErrorDetails>),

    /// Engine error without specific source location
    Engine(String),

    /// Circular dependency error
    CircularDependency(String),

    /// Multiple errors collected together
    MultipleErrors(Vec<LemmaError>),
}

impl LemmaError {
    /// Create a parse error with source information
    pub fn parse(
        message: impl Into<String>,
        span: Span,
        source_id: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
    ) -> Self {
        Self::Parse(Box::new(ErrorDetails {
            message: message.into(),
            span,
            source_id: source_id.into(),
            source_text,
            doc_name: doc_name.into(),
            doc_start_line,
            suggestion: None,
        }))
    }

    /// Create a parse error with suggestion
    pub fn parse_with_suggestion(
        message: impl Into<String>,
        span: Span,
        source_id: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::Parse(Box::new(ErrorDetails {
            message: message.into(),
            span,
            source_id: source_id.into(),
            source_text,
            doc_name: doc_name.into(),
            doc_start_line,
            suggestion: Some(suggestion.into()),
        }))
    }

    /// Create a semantic error with source information
    pub fn semantic(
        message: impl Into<String>,
        span: Span,
        source_id: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
    ) -> Self {
        Self::Semantic(Box::new(ErrorDetails {
            message: message.into(),
            span,
            source_id: source_id.into(),
            source_text,
            doc_name: doc_name.into(),
            doc_start_line,
            suggestion: None,
        }))
    }

    /// Create a semantic error with suggestion
    pub fn semantic_with_suggestion(
        message: impl Into<String>,
        span: Span,
        source_id: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::Semantic(Box::new(ErrorDetails {
            message: message.into(),
            span,
            source_id: source_id.into(),
            source_text,
            doc_name: doc_name.into(),
            doc_start_line,
            suggestion: Some(suggestion.into()),
        }))
    }
}

impl fmt::Display for LemmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LemmaError::Parse(details) => {
                write!(f, "Parse error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {})", suggestion)?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_id, details.span.line, details.span.col
                )
            }
            LemmaError::Semantic(details) => {
                write!(f, "Semantic error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {})", suggestion)?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_id, details.span.line, details.span.col
                )
            }
            LemmaError::Runtime(details) => {
                write!(f, "Runtime error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {})", suggestion)?;
                }
                write!(
                    f,
                    " at {}:{}:{}",
                    details.source_id, details.span.line, details.span.col
                )
            }
            LemmaError::Engine(msg) => write!(f, "Engine error: {}", msg),
            LemmaError::CircularDependency(msg) => write!(f, "Circular dependency: {}", msg),
            LemmaError::MultipleErrors(errors) => {
                writeln!(f, "Multiple errors:")?;
                for (i, error) in errors.iter().enumerate() {
                    write!(f, "  {}. {}", i + 1, error)?;
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
        LemmaError::Engine(format!("Format error: {}", err))
    }
}
