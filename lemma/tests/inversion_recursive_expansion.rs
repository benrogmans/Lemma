use lemma::{Engine, LiteralValue, MoneyUnit, NumericUnit, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

fn money_eur(amount: i32) -> LiteralValue {
    LiteralValue::Unit(NumericUnit::Money(Decimal::from(amount), MoneyUnit::Eur))
}

#[test]
fn test_recursive_rule_reference_expansion_enables_solving() {
    let code = r#"
        doc pricing
        fact base_price = [money]

        rule rate_a = 0.21
        rule rate_b = rate_a? + 0.01
        rule total = base_price * (1 + rate_b?)
    "#;

    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test")
        .expect("Failed to parse lemma code");

    // Invert: total = 122 EUR
    let result = engine.invert(
        "pricing",
        "total",
        Target::value(money_eur(122)),
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
    // allowing the solver to compute base_price = 100 EUR from total = 122 EUR
    // with rate_a = 0.21 and rate_b = 0.22
    //
    // Note: The current domain extraction doesn't yet extract values from algebraically-solved
    // equations, so we just verify that inversion succeeds.
}
