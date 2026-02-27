use ariadne::{Color, Label, Report, ReportKind, Source};
use lemma::error::ErrorDetails;
use lemma::LemmaError;

/// Render an Ariadne error report for any error variant that carries ErrorDetails.
///
/// `error_type` is the human-readable category (e.g. "Parse error", "Engine error").
/// `label_message` is the inline annotation on the source span (empty string for most variants).
fn format_details(error_type: &str, details: &ErrorDetails, label_message: &str) -> String {
    let Some(ref src) = details.source else {
        return format!("{}: {}", error_type, details.message);
    };

    let mut output = Vec::new();

    let header = format!(
        "{}: {} (in doc '{}', file {}:{})",
        error_type, details.message, src.doc_name, src.attribute, src.span.line
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

/// Format a LemmaError with rich terminal output using Ariadne
pub fn format_error(error: &LemmaError) -> String {
    match error {
        LemmaError::Parse(details) => format_details("Parse error", details, ""),
        LemmaError::Semantic(details) => format_details("Semantic error", details, ""),
        LemmaError::Inversion(details) => format_details("Inversion error", details, ""),
        LemmaError::Engine(details) => format_details("Engine error", details, ""),
        LemmaError::MissingFact(details) => format_details("Missing fact", details, ""),
        LemmaError::CircularDependency { details, cycle } => {
            let cycle_note = if cycle.is_empty() {
                String::new()
            } else {
                let path: Vec<String> = cycle
                    .iter()
                    .map(|s| format!("{}:{}", s.doc_name, s.span.line))
                    .collect();
                format!(" [cycle: {}]", path.join(" -> "))
            };
            format_details("Circular dependency", details, &cycle_note)
        }
        LemmaError::Registry {
            details,
            identifier,
            kind,
        } => format_details(
            &format!("Registry error ({})", kind),
            details,
            &format!("@{}", identifier),
        ),
        LemmaError::ResourceLimitExceeded {
            limit_name,
            limit_value,
            actual_value,
            suggestion,
        } => {
            format!(
                "Resource limit exceeded: {limit_name}\n  Limit: {limit_value}\n  Actual: {actual_value}\n  {suggestion}"
            )
        }
        LemmaError::MultipleErrors(errors) => {
            let formatted: Vec<String> = errors.iter().map(format_error).collect();
            formatted.join("\n\n")
        }
    }
}
