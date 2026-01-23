use crate::error::LemmaError;
use crate::parsing::ast::Span;
use std::sync::Arc;

fn create_test_error(
    variant: fn(String, Span, String, Arc<str>, String, usize, Option<String>) -> LemmaError,
) -> LemmaError {
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
        Span { start: 0, end: 0, line: 1, col: 0 },
        "<test>",
        Arc::from(""),
        "<test>",
        1,
        None::<String>,
    );
    assert!(format!("{engine_error}").contains("Engine error: Something went wrong"));

    let circular_dependency_error = LemmaError::circular_dependency(
        "a -> b -> a",
        Span { start: 0, end: 0, line: 1, col: 0 },
        "<test>",
        Arc::from(""),
        "<test>",
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
