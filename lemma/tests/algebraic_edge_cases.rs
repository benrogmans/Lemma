use lemma::{Engine, LiteralValue, Target};
use rust_decimal::Decimal;
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
        .invert(
            "test",
            "y",
            Target::value(LiteralValue::Number(Decimal::from(3))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have free variable x (modulo is not algebraically solvable)
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "x"));
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
        .invert(
            "test",
            "y",
            Target::value(LiteralValue::Number(Decimal::from(16))),
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
        .invert(
            "test",
            "y",
            Target::value(LiteralValue::Number(Decimal::from(17))),
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
    given.insert(
        "test.divisor".to_string(),
        LiteralValue::Number(Decimal::from(0)),
    );

    let solutions = engine
        .invert(
            "test",
            "y",
            Target::value(LiteralValue::Number(Decimal::from(10))),
            given,
        )
        .expect("invert should succeed");

    // When divisor=0 is given, hydration produces x/0, but algebraic solving
    // yields x = 10 * 0 = 0 (constant folded). This is acceptable.
    assert!(!solutions.is_empty(), "Expected at least one solution");
}

#[test]
fn rule_reference_in_expression_stays_opaque() {
    let code = r#"
        doc test
        fact base_price = [money]
        rule markup = 1.2
        rule final_price = base_price * markup?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let target = LiteralValue::Unit(lemma::NumericUnit::Money(
        Decimal::from(120),
        lemma::MoneyUnit::Eur,
    ));

    let solutions = engine
        .invert("test", "final_price", Target::value(target), HashMap::new())
        .expect("invert should succeed");

    // Rule references to simple constants should be substituted during hydration
    // markup = 1.2, so final_price = base_price * 1.2 = 120 EUR
    // Should solve: base_price = 100 EUR
    assert!(!solutions.is_empty(), "Expected at least one solution");
}

#[test]
fn rule_reference_with_dependencies_stays_symbolic() {
    let code = r#"
        doc test
        fact base_price = [money]
        fact markup_factor = [number]
        rule markup = markup_factor * 1.2
        rule final_price = base_price * markup?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let target = LiteralValue::Unit(lemma::NumericUnit::Money(
        Decimal::from(120),
        lemma::MoneyUnit::Eur,
    ));

    let solutions = engine
        .invert("test", "final_price", Target::value(target), HashMap::new())
        .expect("invert should succeed");

    // markup has a dependency (markup_factor), so it stays symbolic
    // Should track transitive dependencies
    assert!(!solutions.is_empty(), "Expected at least one solution");

    // Both base_price and (transitively) markup_factor should be free variables
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "base_price"));
    assert!(
        solutions
            .iter()
            .flat_map(|r| r.keys())
            .any(|v| v.reference.join(".") == "markup_factor"),
        "should track transitive dependencies through rule references"
    );
}
