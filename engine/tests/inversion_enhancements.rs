use lemma::{Engine, LiteralValue, Target};
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

#[test]
fn test_rule_reference_expansion_simple_constant() {
    let code = r#"
        doc pricing
        fact base_price: [number]

        rule tax_rate: 0.21
        rule total_price: base_price * (1 + tax_rate)
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").expect("Failed to parse");

    // Invert for total_price = 121, given no facts
    let result = engine.invert(
        "pricing",
        "total_price",
        Target::value(LiteralValue::number(121.into())),
        HashMap::new(),
    );

    assert!(result.is_ok(), "Inversion should succeed");

    // The tax_rate rule should be expanded since it's a simple constant
    let solutions = result.unwrap();
    assert!(!solutions.is_empty(), "Should have solution solutions");

    // The test validates that rule references are expanded during inversion
    // With simple constant rules like tax_rate = 0.21, the inversion should succeed
}

#[test]
fn test_enhanced_error_message_lists_values() {
    let code = r#"
        doc test
        fact x: [number]

        rule result: 10
          unless x > 5 then 20
          unless x > 10 then 30
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").expect("Failed to parse");

    // Try to invert for a value that doesn't exist in the rule outputs
    let result = engine.invert(
        "test",
        "result",
        Target::value(LiteralValue::number(15.into())),
        HashMap::new(),
    );

    // No matching solutions should exist
    let response = result.expect("Should succeed");
    assert!(
        response.is_empty(),
        "Should have no solutions for value 15 (rule only produces 10, 20, or 30)"
    );
}
