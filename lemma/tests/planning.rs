//! Integration tests for the planning module
//!
//! Tests the planning module end-to-end, including validation,
//! type inference, dependency analysis, and execution plan building.

#![cfg(feature = "planning_internal_tests")]

use lemma::{parse, planning};
use std::collections::HashMap;

#[test]
fn test_basic_validation() {
    let input = r#"doc person
fact name = "John"
fact age = 25
rule is_adult = age >= 18"#;

    let docs = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();

    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), input.to_string());

    for doc in &docs {
        let result = planning::plan(doc, &docs, sources.clone());
        assert!(
            result.is_ok(),
            "Basic validation should pass: {:?}",
            result.err()
        );
    }
}

#[test]
fn test_duplicate_facts() {
    let input = r#"doc person
fact name = "John"
fact name = "Jane""#;

    let docs = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();

    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), input.to_string());

    let result = planning::plan(&docs[0], &docs, sources);

    assert!(
        result.is_err(),
        "Duplicate facts should cause validation error"
    );
    let errors = result.unwrap_err();
    let error_string = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        error_string.contains("Duplicate fact"),
        "Error should mention duplicate fact: {}",
        error_string
    );
    assert!(error_string.contains("name"));
}

#[test]
fn test_duplicate_rules() {
    let input = r#"doc person
fact age = 25
rule is_adult = age >= 18
rule is_adult = age >= 21"#;

    let docs = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();

    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), input.to_string());

    let result = planning::plan(&docs[0], &docs, sources);

    assert!(
        result.is_err(),
        "Duplicate rules should cause validation error"
    );
    let errors = result.unwrap_err();
    let error_string = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        error_string.contains("Duplicate rule"),
        "Error should mention duplicate rule: {}",
        error_string
    );
    assert!(error_string.contains("is_adult"));
}

#[test]
fn test_circular_dependency() {
    let input = r#"doc test
rule a = b?
rule b = a?"#;

    let docs = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();

    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), input.to_string());

    let result = planning::plan(&docs[0], &docs, sources);

    assert!(
        result.is_err(),
        "Circular dependency should cause validation error"
    );
    let errors = result.unwrap_err();
    let error_string = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!("Actual error: {}", error_string);
    assert!(error_string.contains("Circular dependency") || error_string.contains("circular"));
}

#[test]
fn test_reference_type_errors() {
    let input = r#"doc test
fact age = 25
rule is_adult = age >= 18
rule test1 = age?
rule test2 = is_adult"#;

    let docs = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();

    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), input.to_string());

    let result = planning::plan(&docs[0], &docs, sources);

    assert!(
        result.is_err(),
        "Reference type errors should cause validation error"
    );
    let errors = result.unwrap_err();
    let error_string = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        error_string.contains("is a rule, not a fact") || error_string.contains("Reference"),
        "Error should mention reference issue: {}",
        error_string
    );
}

#[test]
fn test_multiple_documents() {
    let input = r#"doc person
fact name = "John"
fact age = 25

doc company
fact name = "Acme Corp"
fact employee = doc person"#;

    let docs = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();

    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), input.to_string());

    let result = planning::plan(&docs[0], &docs, sources);

    assert!(
        result.is_ok(),
        "Multiple documents should validate successfully: {:?}",
        result.err()
    );
}

#[test]
fn test_invalid_document_reference() {
    let input = r#"doc person
fact name = "John"
fact contract = doc nonexistent"#;

    let docs = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();

    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), input.to_string());

    let result = planning::plan(&docs[0], &docs, sources);

    assert!(
        result.is_err(),
        "Invalid document reference should cause validation error"
    );
    let errors = result.unwrap_err();
    let error_string = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        error_string.contains("not found") || error_string.contains("Document"),
        "Error should mention document reference issue: {}",
        error_string
    );
    assert!(error_string.contains("nonexistent"));
}
