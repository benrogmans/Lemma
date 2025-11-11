use lemma::{Engine, LiteralValue, MoneyUnit, NumericUnit, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn inversion_smoke_implicit_single_rule() {
    let code = r#"
        doc pricing
        fact price = [money]
        fact quantity = [number]
        rule total = price * quantity
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert(
        "pricing.quantity".to_string(),
        LiteralValue::Number(Decimal::from(5)),
    );

    let target_value = LiteralValue::Unit(NumericUnit::Money(Decimal::from(50), MoneyUnit::Eur));
    let solutions = engine
        .invert("pricing", "total", Target::value(target_value), given)
        .expect("invert should succeed");

    // Should have at least one solution solution
    // (Algebraically solves for pricing.price = 50 / 5 = 10 EUR)
    assert!(
        !solutions.is_empty(),
        "expected at least one solution solution"
    );
}
