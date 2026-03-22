use ariadne::{Color, Label, Report, ReportKind, Source};
use lemma::error::ErrorDetails;
use lemma::Error;
use std::collections::HashMap;

fn format_details(
    error_type: &str,
    details: &ErrorDetails,
    label_message: &str,
    sources: &HashMap<String, String>,
) -> String {
    let Some(ref src) = details.source else {
        return format!("{}: {}", error_type, details.message);
    };

    let Some(full_content) = sources.get(&src.attribute) else {
        return format!(
            "{}: {} ({}:{})",
            error_type, details.message, src.attribute, src.span.line
        );
    };

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

    let span = (src.attribute.as_str(), src.span.start..src.span.end);
    let mut report = Report::build(ReportKind::Error, span.clone())
        .with_message(header)
        .with_label(
            Label::new(span)
                .with_message(label_message)
                .with_color(Color::Red),
        );

    if let Some(suggestion) = &details.suggestion {
        report = report.with_help(suggestion);
    }

    let content: &str = full_content.as_str();
    if report
        .finish()
        .write((src.attribute.as_str(), Source::from(content)), &mut output)
        .is_err()
    {
        return format!(
            "{}: {} ({}:{})",
            error_type, details.message, src.attribute, src.span.line
        );
    }
    String::from_utf8_lossy(&output).to_string()
}

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
