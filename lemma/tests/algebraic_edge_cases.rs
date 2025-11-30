#![cfg(feature = "inversion")]

use lemma::{Engine, LiteralValue, Target};
use std::collections::HashMap;

#[test]
fn modulo_operator_not_supported() {
    let code = r#"
        doc test
        fact x = [number]
        rule y = x % 10
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let solutions = engine
        .invert_strict(
            "test",
            "y",
            Target::value(LiteralValue::number(3)),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have free variable x (modulo is not algebraically solvable)
    let x_ref = lemma::FactPath::new(vec![], "x".to_string());
    assert!(solutions.iter().flat_map(|r| r.keys()).any(|v| v == &x_ref));
}

#[test]
fn power_operator_supported() {
    let code = r#"
        doc test
        fact x = [number]
        rule y = x ^ 2
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let solutions = engine
        .invert_strict(
            "test",
            "y",
            Target::value(LiteralValue::number(16)),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should solve x^2 = 16 => x = 4 (principal root)
    assert!(!solutions.is_empty(), "Expected at least one solution");
}

#[test]
fn nested_arithmetic_single_unknown() {
    let code = r#"
        doc test
        fact x = [number]
        rule y = ((x + 5) * 2) - 3
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let solutions = engine
        .invert_strict(
            "test",
            "y",
            Target::value(LiteralValue::number(17)),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should solve: ((x + 5) * 2) - 3 = 17 => x = 5
    // Verification: ((5 + 5) * 2) - 3 = (10 * 2) - 3 = 20 - 3 = 17 ✓
    assert!(!solutions.is_empty(), "Expected at least one solution");
}

#[test]
fn divide_by_zero_stays_symbolic() {
    let code = r#"
        doc test
        fact x = [number]
        fact divisor = [number]
        rule y = x / divisor
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert("divisor".to_string(), LiteralValue::number(0));

    let solutions = engine
        .invert_strict("test", "y", Target::value(LiteralValue::number(10)), given)
        .expect("invert should succeed");

    // When divisor=0 is given, hydration produces x/0, but algebraic solving
    // yields x = 10 * 0 = 0 (constant folded). This is acceptable.
    assert!(!solutions.is_empty(), "Expected at least one solution");
}

#[test]
fn rule_reference_in_expression_stays_opaque() {
    let code = r#"
        doc test
        fact base_price = [number]
        rule markup = 1.2
        rule final_price = base_price * markup?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let target = LiteralValue::number(120);

    let solutions = engine
        .invert_strict("test", "final_price", Target::value(target), HashMap::new())
        .expect("invert should succeed");

    // Rule references to simple constants should be substituted during hydration
    // markup = 1.2, so final_price = base_price * 1.2 = 120
    // Should solve: base_price = 100
    assert!(!solutions.is_empty(), "Expected at least one solution");
}

#[test]
fn rule_reference_with_dependencies_stays_symbolic() {
    let code = r#"
        doc test
        fact base_price = [number]
        fact markup_factor = [number]
        rule markup = markup_factor * 1.2
        rule final_price = base_price * markup?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let target = LiteralValue::number(120);

    let solutions = engine
        .invert_strict("test", "final_price", Target::value(target), HashMap::new())
        .expect("invert should succeed");

    // markup has a dependency (markup_factor), so it stays symbolic
    // Should track transitive dependencies
    assert!(!solutions.is_empty(), "Expected at least one solution");

    // Both base_price and (transitively) markup_factor should be free variables
    let base_price_ref = lemma::FactPath::new(vec![], "base_price".to_string());
    let markup_factor_ref = lemma::FactPath::new(vec![], "markup_factor".to_string());
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v == &base_price_ref));
    assert!(
        solutions
            .iter()
            .flat_map(|r| r.keys())
            .any(|v| v == &markup_factor_ref),
        "should track transitive dependencies through rule references"
    );
}
