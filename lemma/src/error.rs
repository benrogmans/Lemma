use crate::ast::Span;
use ariadne::{Color, Label, Report, ReportKind, Source};
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

    /// Rule blocked by veto with optional message
    Veto(Option<String>),

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
                let context = ErrorContext {
                    error_type: "Parse error",
                    message: &details.message,
                    span: &details.span,
                    source_id: &details.source_id,
                    source_text: &details.source_text,
                    doc_name: &details.doc_name,
                    doc_start_line: details.doc_start_line,
                    suggestion: details.suggestion.as_deref(),
                    note: None,
                };
                format_error_with_ariadne(f, &context)
            }
            LemmaError::Semantic(details) => {
                let context = ErrorContext {
                    error_type: "Semantic error",
                    message: &details.message,
                    span: &details.span,
                    source_id: &details.source_id,
                    source_text: &details.source_text,
                    doc_name: &details.doc_name,
                    doc_start_line: details.doc_start_line,
                    suggestion: details.suggestion.as_deref(),
                    note: None,
                };
                format_error_with_ariadne(f, &context)
            }
            LemmaError::Runtime(details) => {
                let context = ErrorContext {
                    error_type: "Runtime error",
                    message: &details.message,
                    span: &details.span,
                    source_id: &details.source_id,
                    source_text: &details.source_text,
                    doc_name: &details.doc_name,
                    doc_start_line: details.doc_start_line,
                    suggestion: details.suggestion.as_deref(),
                    note: None,
                };
                format_error_with_ariadne(f, &context)
            }
            LemmaError::Engine(msg) => write!(f, "Engine error: {}", msg),
            LemmaError::CircularDependency(msg) => write!(f, "Circular dependency: {}", msg),
            LemmaError::Veto(msg_opt) => match msg_opt {
                Some(msg) => write!(f, "Rule blocked by veto: {}", msg),
                None => write!(f, "Rule blocked by veto"),
            },
            LemmaError::MultipleErrors(errors) => {
                writeln!(f, "Multiple errors occurred:")?;
                for error in errors {
                    writeln!(f, "\n{}", error)?;
                }
                Ok(())
            }
        }
    }
}

struct ErrorContext<'a> {
    error_type: &'a str,
    message: &'a str,
    span: &'a Span,
    source_id: &'a str,
    source_text: &'a str,
    doc_name: &'a str,
    doc_start_line: usize,
    suggestion: Option<&'a str>,
    note: Option<&'a str>,
}

fn format_error_with_ariadne(f: &mut fmt::Formatter<'_>, context: &ErrorContext) -> fmt::Result {
    let mut output = Vec::new();

    let doc_line = if context.span.line >= context.doc_start_line {
        context.span.line - context.doc_start_line + 1
    } else {
        context.span.line // Fallback if something is off
    };

    // Enhanced error message showing both doc and file context
    let enhanced_message = format!(
        "{}: {} (in doc '{}' at line {}, file {}:{})",
        context.error_type,
        context.message,
        context.doc_name,
        doc_line,
        context.source_id,
        context.span.line
    );

    let mut report = Report::build(ReportKind::Error, context.source_id, context.span.start)
        .with_message(enhanced_message)
        .with_label(
            Label::new((context.source_id, context.span.start..context.span.end))
                .with_message("")
                .with_color(Color::Red),
        );

    if let Some(help) = context.suggestion {
        report = report.with_help(help);
    }

    if let Some(note_text) = context.note {
        report = report.with_note(note_text);
    }

    match report.finish().write(
        (context.source_id, Source::from(context.source_text)),
        &mut output,
    ) {
        Ok(_) => write!(f, "{}", String::from_utf8_lossy(&output)),
        Err(_) => {
            // Fallback if ariadne fails
            write!(
                f,
                "{}: {} at {}:{}:{}",
                context.error_type,
                context.message,
                context.source_id,
                context.span.line,
                context.span.col
            )
        }
    }
}

impl std::error::Error for LemmaError {}

impl From<std::fmt::Error> for LemmaError {
    fn from(err: std::fmt::Error) -> Self {
        LemmaError::Engine(format!("Format error: {}", err))
    }
}
