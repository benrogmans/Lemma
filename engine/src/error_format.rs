//! Error formatting with Ariadne. Sources are required when the error has a source location.

use crate::error::ErrorDetails;
use crate::Error;
use ariadne::{Color, Label, Report, ReportKind, Source};
use std::collections::HashMap;
use std::fmt;

/// Render an Ariadne error report. Sources must contain all attributes referenced by errors.
fn format_details(
    error_type: &str,
    details: &ErrorDetails,
    label_message: &str,
    sources: &HashMap<String, String>,
) -> String {
    let Some(ref src) = details.source else {
        return format!("{}: {}", error_type, details.message);
    };

    let full_content = sources.get(&src.attribute).unwrap_or_else(|| {
        unreachable!(
            "invariant: sources must contain attribute {} for error display",
            src.attribute
        )
    });

    let mut output = Vec::new();

    let header = match details.spec_context.as_ref() {
        Some(spec) => format!(
            "{}: {} (in spec '{}', file {}:{})",
            error_type, details.message, spec.name, src.attribute, src.span.line
        ),
        None => format!(
            "{}: {} ({}:{})",
            error_type, details.message, src.attribute, src.span.line
        ),
    };

    let mut report = Report::build(ReportKind::Error, &src.attribute, src.span.start)
        .with_message(header)
        .with_label(
            Label::new((&src.attribute, src.span.start..src.span.end))
                .with_message(label_message)
                .with_color(Color::Red),
        );

    if let Some(suggestion) = &details.suggestion {
        report = report.with_help(suggestion);
    }

    let content: &str = full_content.as_str();
    report
        .finish()
        .write((&src.attribute, Source::from(content)), &mut output)
        .unwrap_or_else(|e| panic!("Ariadne report write failed: {}", e));
    String::from_utf8_lossy(&output).to_string()
}

/// Format a Lemma Error with rich terminal output. Sources are required.
#[must_use]
pub fn format_error(error: &Error, sources: &HashMap<String, String>) -> String {
    let fmt = |typ: &str, details: &ErrorDetails, label: &str| {
        format_details(typ, details, label, sources)
    };
    match error {
        Error::Parsing(details) => fmt("Parse error", details, ""),
        Error::Inversion(details) => fmt("Inversion error", details, ""),
        Error::Validation(details) => fmt("Validation error", details, ""),
        Error::Registry {
            details,
            identifier,
            kind,
        } => fmt(&format!("Registry error ({})", kind), details, identifier),
        Error::ResourceLimitExceeded {
            details,
            limit_name,
            limit_value,
            actual_value,
        } => fmt(
            &format!("Resource limit exceeded: {limit_name} (limit: {limit_value}, actual: {actual_value})"),
            details,
            "",
        ),
        Error::Request { details, .. } => fmt("Request error", details, ""),
    }
}

/// Load failure: errors plus the source files we attempted to load.
#[derive(Debug, Clone)]
pub struct LoadError {
    pub errors: Vec<Error>,
    pub sources: HashMap<String, String>,
}

impl LoadError {
    #[must_use]
    pub fn format_all(&self) -> Vec<String> {
        self.errors
            .iter()
            .map(|e| format_error(e, &self.sources))
            .collect()
    }

    /// Iterate over the errors.
    pub fn iter(&self) -> std::slice::Iter<'_, Error> {
        self.errors.iter()
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_all().join("\n\n"))
    }
}
