use lemma::{Engine, LiteralValue, MoneyUnit, NumericUnit, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

fn money_eur(amount: i32) -> LiteralValue {
    LiteralValue::Unit(NumericUnit::Money(Decimal::from(amount), MoneyUnit::Eur))
}

fn number(amount: i32) -> LiteralValue {
    LiteralValue::Number(Decimal::from(amount))
}

#[test]
fn test_rule_reference_expansion_simple_constant() {
    let code = r#"
        doc pricing
        fact base_price = [money]

        rule tax_rate = 0.21
        rule total_price = base_price * (1 + tax_rate?)
    "#;

    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test")
        .expect("Failed to parse");

    // Invert for total_price = 121 EUR, given no facts
    let result = engine.invert(
        "pricing",
        "total_price",
        Target::value(money_eur(121)),
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
        fact x = [number]

        rule result = 10
          unless x > 5 then 20
          unless x > 10 then 30
    "#;

    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test")
        .expect("Failed to parse");

    // Try to invert for a value that doesn't exist
    let result = engine.invert("test", "result", Target::value(number(15)), HashMap::new());

    assert!(result.is_err(), "Should fail for non-existent value");

    let err = result.unwrap_err();
    let err_msg = format!("{}", err);

    // Error should list what values ARE producible
    assert!(
        err_msg.contains("This rule can produce"),
        "Should list available outcomes: {}",
        err_msg
    );
    assert!(
        err_msg.contains("10") || err_msg.contains("20") || err_msg.contains("30"),
        "Should mention at least one actual value: {}",
        err_msg
    );
}
