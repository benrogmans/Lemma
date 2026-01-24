use ariadne::{Color, Label, Report, ReportKind, Source};
use lemma::LemmaError;

/// Format a LemmaError with fancy terminal output using Ariadne
pub fn format_error(error: &LemmaError) -> String {
    match error {
        LemmaError::Parse(details)
        | LemmaError::Semantic(details)
        | LemmaError::Inversion(details)
        | LemmaError::Runtime(details) => {
            let mut output = Vec::new();

            let error_type = match error {
                LemmaError::Parse(_) => "Parse error",
                LemmaError::Semantic(_) => "Semantic error",
                LemmaError::Inversion(_) => "Inversion error",
                LemmaError::Runtime(_) => "Runtime error",
                _ => unreachable!(),
            };

            let doc_line = if details.source_location.span.line >= details.doc_start_line {
                details.source_location.span.line - details.doc_start_line + 1
            } else {
                details.source_location.span.line
            };

            let enhanced_message = format!(
                "{error_type}: {} (in doc '{}' at line {}, file {}:{})",
                details.message,
                details.source_location.doc_name,
                doc_line,
                details.source_location.attribute,
                details.source_location.span.line
            );

            let mut report = Report::build(
                ReportKind::Error,
                &details.source_location.attribute,
                details.source_location.span.start,
            )
            .with_message(enhanced_message)
            .with_label(
                Label::new((
                    &details.source_location.attribute,
                    details.source_location.span.start..details.source_location.span.end,
                ))
                .with_message("")
                .with_color(Color::Red),
            );

            if let Some(suggestion) = &details.suggestion {
                report = report.with_help(suggestion);
            }

            match report.finish().write(
                (
                    &details.source_location.attribute,
                    Source::from(details.source_text.as_ref()),
                ),
                &mut output,
            ) {
                Ok(_) => String::from_utf8_lossy(&output).to_string(),
                Err(_) => {
                    // Fallback to simple format
                    format!("{}", error)
                }
            }
        }
        LemmaError::Engine(details) => format!("Engine error: {}", details.message),
        LemmaError::MissingFact(details) => format!("Missing fact: {}", details.message),
        LemmaError::CircularDependency { details, .. } => {
            format!("Circular dependency: {}", details.message)
        }
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
            let mut result = String::from("Multiple errors occurred:\n\n");
            for error in errors {
                result.push_str(&format_error(error));
                result.push_str("\n\n");
            }
            result
        }
    }
}
