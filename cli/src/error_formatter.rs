use ariadne::{Color, Label, Report, ReportKind, Source};
use lemma::LemmaError;

/// Format a LemmaError with fancy terminal output using Ariadne
pub fn format_error(error: &LemmaError) -> String {
    match error {
        LemmaError::Parse(details)
        | LemmaError::Semantic(details)
        | LemmaError::Runtime(details) => {
            let Some(source) = &details.source else {
                return format!("{error}");
            };

            let mut output = Vec::new();

            let error_type = match error {
                LemmaError::Parse(_) => "Parse error",
                LemmaError::Semantic(_) => "Semantic error",
                LemmaError::Runtime(_) => "Runtime error",
                _ => unreachable!(),
            };

            let doc_line = if source.span.line >= details.doc_start_line {
                source.span.line - details.doc_start_line + 1
            } else {
                source.span.line
            };

            let enhanced_message = format!(
                "{error_type}: {} (in doc '{}' at line {}, file {}:{})",
                details.message, source.doc_name, doc_line, source.source_id, source.span.line
            );

            let mut report = Report::build(ReportKind::Error, &source.source_id, source.span.start)
                .with_message(enhanced_message)
                .with_label(
                    Label::new((&source.source_id, source.span.start..source.span.end))
                        .with_message("")
                        .with_color(Color::Red),
                );

            if let Some(suggestion) = &details.suggestion {
                report = report.with_help(suggestion);
            }

            match report.finish().write(
                (
                    &source.source_id,
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
        LemmaError::Engine(msg) => format!("Engine error: {msg}"),
        LemmaError::MissingFact(fact_ref) => format!("Missing fact: {fact_ref}"),
        LemmaError::CircularDependency(msg) => format!("Circular dependency: {msg}"),
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
