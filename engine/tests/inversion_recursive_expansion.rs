use lemma::{Engine, LiteralValue, Target};
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use std::collections::HashMap;

#[test]
fn test_recursive_rule_reference_expansion_enables_solving() {
    let code = r#"
        spec pricing
        fact base_price: [number]

        rule rate_a: 0.21
        rule rate_b: rate_a + 0.01
        rule total: base_price * (1 + rate_b)
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").expect("Failed to parse lemma code");

    // Invert: total = 122
    let now = DateTimeValue::now();
    let result = engine.invert(
        "pricing",
        &now,
        "total",
        Target::value(LiteralValue::number(122.into())),
        HashMap::new(),
    );
    assert!(result.is_ok(), "Inversion should succeed: {:?}", result);

    let solutions = result.unwrap();

    // Should have solution solutions returned
    assert!(
        !solutions.is_empty(),
        "Expected at least one solution solution"
    );

    // For fully-solved single-unknown cases, the algebraic solver determines the exact value
    // The test validates that recursive rule expansion happens during inversion,
    // allowing the solver to compute base_price = 100 from total = 122
    // with rate_a = 0.21 and rate_b = 0.22
    //
    // Note: The current domain extraction doesn't yet extract values from algebraically-solved
    // equations, so we just verify that inversion succeeds.
}
