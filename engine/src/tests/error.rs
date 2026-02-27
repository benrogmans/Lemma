use crate::error::LemmaError;
use crate::parsing::ast::Span;
use crate::parsing::source::Source;
use std::sync::Arc;

fn test_source() -> Source {
    let source_text = "fact amount = 100";
    Source::new(
        "test.lemma",
        Span {
            start: 14,
            end: 21,
            line: 1,
            col: 15,
        },
        "test_doc",
        Arc::from(source_text),
    )
}

#[test]
fn test_error_creation_and_display() {
    let source = test_source();

    let parse_error = LemmaError::parse("Invalid currency", Some(source.clone()), None::<String>);
    assert_eq!(
        format!("{parse_error}"),
        "Parse error: Invalid currency at test.lemma:1:15"
    );

    let typo_source_text = "fact amont = 100";
    let typo_source = Source::new(
        "suggestion.lemma",
        Span {
            start: 5,
            end: 10,
            line: 1,
            col: 6,
        },
        "suggestion_doc",
        Arc::from(typo_source_text),
    );

    let parse_error_with_suggestion = LemmaError::parse_with_suggestion(
        "Typo in fact name",
        Some(typo_source),
        "Did you mean 'amount'?",
    );
    assert_eq!(
        format!("{parse_error_with_suggestion}"),
        "Parse error: Typo in fact name (suggestion: Did you mean 'amount'?) at suggestion.lemma:1:6"
    );

    let engine_error =
        LemmaError::engine("Something went wrong", Some(source.clone()), None::<String>);
    assert_eq!(
        format!("{engine_error}"),
        "Engine error: Something went wrong at test.lemma:1:15"
    );

    let circular_dependency_error =
        LemmaError::circular_dependency("a -> b -> a", Some(source), vec![], None::<String>);
    assert_eq!(
        format!("{circular_dependency_error}"),
        "Circular dependency: a -> b -> a at test.lemma:1:15"
    );

    let engine_error_no_source = LemmaError::engine("No source context", None, None::<String>);
    assert_eq!(
        format!("{engine_error_no_source}"),
        "Engine error: No source context"
    );

    let multiple_errors = LemmaError::MultipleErrors(vec![parse_error, engine_error_no_source]);
    assert_eq!(
        format!("{multiple_errors}"),
        "Multiple errors:\n  1. Parse error: Invalid currency at test.lemma:1:15\n  2. Engine error: No source context"
    );
}
