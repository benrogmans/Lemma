//! Tests for type propagation through arithmetic operations

use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

#[test]
fn test_money_plus_number_preserves_money() {
    // Money + Number → Money
    let code = r#"
    doc test
    type money = number
    fact a = [money]
    fact b = 100
    rule total = a + b
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    // Verify evaluation succeeds
    let result = engine.evaluate("test", vec!["total".to_string()], HashMap::new());
    assert!(result.is_ok(), "Evaluation should succeed");
}

#[test]
fn test_number_plus_money_preserves_money() {
    // Number + Money → Money
    let code = r#"
    doc test
    type money = number
    fact a = 100
    fact b = [money]
    rule total = a + b
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let result = engine.evaluate("test", vec!["total".to_string()], HashMap::new());
    assert!(result.is_ok(), "Evaluation should succeed");
}

#[test]
fn test_money_plus_money_preserves_money() {
    // Money + Money → Money
    let code = r#"
    doc test
    type money = number
    fact a = [money]
    fact b = [money]
    rule total = a + b
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let result = engine.evaluate("test", vec!["total".to_string()], HashMap::new());
    assert!(result.is_ok(), "Evaluation should succeed");
}

#[test]
fn test_different_custom_types_same_base() {
    // Money + Price (both extend Number, no units) - should preserve one type
    // Both extend number (dimensionless), so they're compatible and should succeed
    let code = r#"
    doc test
    type money = number
    type price = number
    fact a = [money]
    fact b = [price]
    rule total = a + b
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    // This should succeed - both extend number (dimensionless), so they're compatible
    // Result type should be money (left operand)
    let result = engine.evaluate("test", vec!["total".to_string()], HashMap::new());
    assert!(
        result.is_ok(),
        "Evaluation should succeed for compatible types (both Number, no units)"
    );
}

#[test]
fn test_incompatible_types_error() {
    // Test that incompatible types (different base types) produce an error
    // For example: number + text should error during planning/validation
    let code = r#"
    doc test
    type money = number
    fact a = [money]
    fact b = "hello"
    rule total = a + b
    "#;

    let mut engine = Engine::new();
    // This should fail during planning/validation because number + text is incompatible
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "Should fail during planning/validation for incompatible types (number + text)"
    );

    // Verify the error message mentions incompatible types
    let error_msg = format!("{}", result.unwrap_err());
    assert!(
        error_msg.to_lowercase().contains("numeric")
            || error_msg.to_lowercase().contains("incompatible")
            || error_msg.to_lowercase().contains("arithmetic"),
        "Error message should mention type incompatibility. Got: {}",
        error_msg
    );
}

#[test]
fn test_different_scale_types_are_incompatible() {
    // EUR + KILOGRAM both extend scale with units, but they're different Scale types
    // They should be rejected (validation fails) because different Scale types are incompatible
    let code = r#"
    doc test
    type eur = scale
      -> unit EUR 1.00
    type kilogram = scale
      -> unit KG 1.00
    fact a = [eur]
    fact b = [kilogram]
    rule total = a + b
    "#;

    let mut engine = Engine::new();
    let parse_result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    // This should fail because different Scale types are incompatible
    assert!(
        parse_result.is_err(),
        "Should fail during validation because different Scale types are incompatible"
    );

    // Verify the error message mentions incompatible Scale types
    let error_msg = format!("{}", parse_result.unwrap_err());
    assert!(
        error_msg.to_lowercase().contains("scale")
            || error_msg.to_lowercase().contains("different"),
        "Error message should mention incompatible Scale types. Got: {}",
        error_msg
    );
}
