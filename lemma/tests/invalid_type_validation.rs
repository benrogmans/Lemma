//! Tests to ensure invalid types are caught during validation, not silently ignored

use lemma::planning::TypeRegistry;
use lemma::{parse, Engine, ResourceLimits};

#[test]
fn test_invalid_parent_type_in_named_type() {
    // Test that invalid parent types in named type definitions are caught
    let code = r#"doc test
type invalid = nonexistent_type -> minimum 0"#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    let doc = &docs[0];

    let mut registry = TypeRegistry::new();
    registry
        .register_type(&doc.name, doc.types[0].clone())
        .unwrap();

    let result = registry.resolve_types(&doc.name);
    assert!(result.is_err(), "Should reject invalid parent type");

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Unknown type") && error_msg.contains("nonexistent_type"),
        "Error should mention unknown type. Got: {}",
        error_msg
    );
}

#[test]
fn test_invalid_parent_type_in_anonymous_type() {
    // Test that invalid parent types in anonymous type definitions are caught
    let code = r#"doc test
fact value = [invalid_parent_type]"#;

    let _docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    // Anonymous types are registered during graph building, not here
    // The error will be caught when resolving types with include_anonymous=true
    // For now, just verify the code parses (the error will be caught during planning)
}

#[test]
fn test_invalid_standard_type_name() {
    // Test that invalid standard type names are caught
    let code = r#"doc test
type invalid = choice -> option "a""#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    let doc = &docs[0];

    let mut registry = TypeRegistry::new();
    registry
        .register_type(&doc.name, doc.types[0].clone())
        .unwrap();

    let result = registry.resolve_types(&doc.name);
    assert!(
        result.is_err(),
        "Should reject invalid standard type 'choice'"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Unknown type") && error_msg.contains("choice"),
        "Error should mention unknown type 'choice'. Got: {}",
        error_msg
    );
    assert!(
        error_msg.contains("Valid standard types"),
        "Error should list valid standard types. Got: {}",
        error_msg
    );
}

#[test]
fn test_invalid_type_via_engine() {
    // Test that invalid types are caught when using Engine
    let code = r#"doc test
type invalid = nonexistent -> minimum 0
fact value = [invalid]
rule result = value"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Engine should reject document with invalid parent type"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Unknown type") || error_msg.contains("nonexistent"),
        "Error should mention unknown type. Got: {}",
        error_msg
    );
}
