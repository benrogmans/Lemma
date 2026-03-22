//! Tests for type propagation through arithmetic operations

use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

#[test]
fn test_money_plus_number_preserves_money() {
    // Money + Number → Money
    let code = r#"
    spec test
    type money: number
    fact a: [money]
    fact b: 100
    rule total: a + b
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    let mut response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    response.filter_rules(&[String::from("total")]);
    assert_eq!(response.results.len(), 1);
}

#[test]
fn test_number_plus_money_preserves_money() {
    // Number + Money → Money
    let code = r#"
    spec test
    type money: number
    fact a: 100
    fact b: [money]
    rule total: a + b
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    let mut response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    response.filter_rules(&[String::from("total")]);
    assert_eq!(response.results.len(), 1);
}

#[test]
fn test_money_plus_money_preserves_money() {
    // Money + Money → Money
    let code = r#"
    spec test
    type money: number
    fact a: [money]
    fact b: [money]
    rule total: a + b
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    let mut response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    response.filter_rules(&[String::from("total")]);
    assert_eq!(response.results.len(), 1);
}

#[test]
fn test_different_custom_types_same_base() {
    // Money + Price (both extend Number, no units) - should preserve one type
    // Both extend number (dimensionless), so they're compatible and should succeed
    let code = r#"
    spec test
    type money: number
    type price: number
    fact a: [money]
    fact b: [price]
    rule total: a + b
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    // This should succeed - both extend number (dimensionless), so they're compatible
    let mut response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    response.filter_rules(&[String::from("total")]);
    assert_eq!(response.results.len(), 1);
}

#[test]
fn test_incompatible_types_error() {
    // Test that incompatible types (different base types) produce an error
    // For example: number + text should error during planning/validation
    let code = r#"
    spec test
    type money: number
    fact a: [money]
    fact b: "hello"
    rule total: a + b
    "#;

    let mut engine = Engine::new();
    // This should fail during planning/validation because number + text is incompatible
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "Should fail during planning/validation for incompatible types (number + text)"
    );

    // Verify the error message mentions incompatible types
    let errs = result.unwrap_err();
    let error_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        error_msg.contains("Cannot apply"),
        "Error message should mention invalid operation. Got: {}",
        error_msg
    );
}

#[test]
fn test_different_scale_types_are_incompatible() {
    // EUR + KILOGRAM both extend scale with units, but they're different Scale types
    // They should be rejected (validation fails) because different Scale types are incompatible
    let code = r#"
    spec test
    type eur: scale
      -> unit EUR 1.00
    type kilogram: scale
      -> unit KG 1.00
    fact a: [eur]
    fact b: [kilogram]
    rule total: a + b
    "#;

    let mut engine = Engine::new();
    let parse_result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    // This should fail because different Scale types are incompatible
    assert!(
        parse_result.is_err(),
        "Should fail during validation because different Scale types are incompatible"
    );

    // Verify the error message mentions incompatible Scale types
    let errs = parse_result.unwrap_err();
    let error_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        error_msg.to_lowercase().contains("scale")
            || error_msg.to_lowercase().contains("different"),
        "Error message should mention incompatible Scale types. Got: {}",
        error_msg
    );
}
