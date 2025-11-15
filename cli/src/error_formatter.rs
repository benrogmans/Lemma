use ariadne::{Color, Label, Report, ReportKind, Source};
use lemma::LemmaError;

/// Format a LemmaError with fancy terminal output using Ariadne
pub fn format_error(error: &LemmaError) -> String {
    match error {
        LemmaError::Parse(details)
        | LemmaError::Semantic(details)
        | LemmaError::Runtime(details) => {
            let mut output = Vec::new();

            let error_type = match error {
                LemmaError::Parse(_) => "Parse error",
                LemmaError::Semantic(_) => "Semantic error",
                LemmaError::Runtime(_) => "Runtime error",
                _ => unreachable!(),
            };

            let doc_line = if details.span.line >= details.doc_start_line {
                details.span.line - details.doc_start_line + 1
            } else {
                details.span.line
            };

            let enhanced_message = format!(
                "{}: {} (in doc '{}' at line {}, file {}:{})",
                error_type,
                details.message,
                details.doc_name,
                doc_line,
                details.source_id,
                details.span.line
            );

            let mut report =
                Report::build(ReportKind::Error, &details.source_id, details.span.start)
                    .with_message(enhanced_message)
                    .with_label(
                        Label::new((&details.source_id, details.span.start..details.span.end))
                            .with_message("")
                            .with_color(Color::Red),
                    );

            if let Some(suggestion) = &details.suggestion {
                report = report.with_help(suggestion);
            }

            match report.finish().write(
                (
                    &details.source_id,
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
        LemmaError::Engine(msg) => format!("Engine error: {}", msg),
        LemmaError::MissingFact(fact_ref) => format!("Missing fact: {}", fact_ref),
        LemmaError::CircularDependency(msg) => format!("Circular dependency: {}", msg),
        LemmaError::ResourceLimitExceeded {
            limit_name,
            limit_value,
            actual_value,
            suggestion,
        } => {
            format!(
                "Resource limit exceeded: {}\n  Limit: {}\n  Actual: {}\n  {}",
                limit_name, limit_value, actual_value, suggestion
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
