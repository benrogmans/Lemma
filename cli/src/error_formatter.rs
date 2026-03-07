use ariadne::{Color, Label, Report, ReportKind, Source};
use lemma::error::ErrorDetails;
use lemma::Error;

/// Render an Ariadne error report for any error variant that carries ErrorDetails.
///
/// `error_type` is the human-readable category (e.g. "Parse error", "Planning error").
/// `label_message` is the inline annotation on the source span (empty string for most variants).
fn format_details(error_type: &str, details: &ErrorDetails, label_message: &str) -> String {
    let Some(ref src) = details.source else {
        return format!("{}: {}", error_type, details.message);
    };

    let mut output = Vec::new();

    let header = format!(
        "{}: {} (in spec '{}', file {}:{})",
        error_type, details.message, src.spec_name, src.attribute, src.span.line
    );

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

    match report.finish().write(
        (&src.attribute, Source::from(src.source_text.as_ref())),
        &mut output,
    ) {
        Ok(()) => String::from_utf8_lossy(&output).to_string(),
        Err(_) => format!(
            "{}: {} at {}:{}:{}",
            error_type, details.message, src.attribute, src.span.line, src.span.col
        ),
    }
}

/// Format a Error with rich terminal output using Ariadne
pub fn format_error(error: &Error) -> String {
    match error {
        Error::Parsing(details) => format_details("Parse error", details, ""),
        Error::Inversion(details) => format_details("Inversion error", details, ""),
        Error::Validation(details) => format_details("Validation error", details, ""),
        Error::Registry {
            details,
            identifier,
            kind,
        } => format_details(
            &format!("Registry error ({})", kind),
            details,
            &format!("@{}", identifier),
        ),
        Error::ResourceLimitExceeded {
            limit_name,
            limit_value,
            actual_value,
            suggestion,
            spec_context: _,
        } => {
            format!(
                "Resource limit exceeded: {limit_name}\n  Limit: {limit_value}\n  Actual: {actual_value}\n  {suggestion}"
            )
        }
        Error::Request(details) => format_details("Request error", details, ""),
    }
}
