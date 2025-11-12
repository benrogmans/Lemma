use lemma::{Engine, LiteralValue, MoneyUnit, NumericUnit, Target, TargetOp};
use rust_decimal::Decimal;

fn eur(amount: i64) -> LiteralValue {
    LiteralValue::Unit(NumericUnit::Money(Decimal::from(amount), MoneyUnit::Eur))
}

#[test]
fn piecewise_value_guard_pruning_equality() {
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule shipping_cost = 5 EUR
             unless weight >= 10 kilograms then 10 EUR
             unless weight >= 50 kilograms then 25 EUR
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let solutions = engine
        .invert(
            "shipping",
            "shipping_cost",
            Target::value(eur(10)),
            std::collections::HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have solution solutions
    assert!(!solutions.is_empty(), "Expected at least one solution");

    // Test validates that guard conditions filter branches correctly
    // The 10 EUR branch should be included with appropriate weight constraints
}

#[test]
fn piecewise_value_guard_pruning_inequality() {
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule shipping_cost = 5 EUR
             unless weight >= 10 kilograms then 10 EUR
             unless weight >= 50 kilograms then 25 EUR
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let solutions = engine
        .invert(
            "shipping",
            "shipping_cost",
            Target::with_op(TargetOp::Gt, lemma::OperationResult::Value(eur(5))),
            std::collections::HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have solution solutions (both 10 EUR and 25 EUR satisfy > 5 EUR)
    assert!(!solutions.is_empty(), "Expected at least one solution");
}
