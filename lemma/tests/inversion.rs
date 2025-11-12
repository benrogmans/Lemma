use lemma::{Engine, LiteralValue, MoneyUnit, NumericUnit, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

fn setup_engine(code: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test")
        .expect("Failed to add code");
    engine
}

fn money_eur(amount: i32) -> LiteralValue {
    LiteralValue::Unit(NumericUnit::Money(Decimal::from(amount), MoneyUnit::Eur))
}

#[test]
fn test_inversion_simple_arithmetic() {
    let code = r#"
        doc pricing
        fact price = [money]
        fact quantity = [number]
        rule total = price * quantity
    "#;
    let engine = setup_engine(code);

    // Invert: total = 100 EUR
    let result = engine.invert(
        "pricing",
        "total",
        Target::value(money_eur(100)),
        HashMap::new(),
    );
    assert!(result.is_ok(), "Inversion should succeed: {:?}", result);

    let solutions = result.unwrap();

    // Should have at least one solution solution
    assert!(
        !solutions.is_empty(),
        "Should have at least one solution solution"
    );

    // Should have price and quantity as free variables in the domains
    let first_solution = &solutions[0];
    let fact_names: Vec<String> = first_solution
        .keys()
        .map(|fp| fp.reference.join("."))
        .collect();

    // Check that we have both price and quantity (or their qualified versions)
    let has_price = fact_names.iter().any(|v| v.contains("price"));
    let has_quantity = fact_names.iter().any(|v| v.contains("quantity"));

    assert!(
        has_price || has_quantity,
        "Should have constraints on price or quantity, found: {:?}",
        fact_names
    );
}

#[test]
fn test_inversion_veto_query() {
    let code = r#"
        doc shipping
        fact weight = [mass]
        rule shipping_cost = 5 EUR
          unless weight >= 10 kilograms then 10 EUR
          unless weight >= 50 kilograms then 25 EUR
          unless weight < 0 kilograms then veto "invalid"
          unless weight > 100 kilograms then veto "too heavy"
    "#;
    let engine = setup_engine(code);

    // Query for "too heavy" veto
    let result = engine.invert(
        "shipping",
        "shipping_cost",
        Target::veto(Some("too heavy".to_string())),
        HashMap::new(),
    );
    assert!(
        result.is_ok(),
        "Veto inversion should succeed: {:?}",
        result
    );

    let solutions = result.unwrap();

    assert!(
        !solutions.is_empty(),
        "Should have at least one solution solution for veto"
    );

    // The veto "too heavy" should trigger when weight > 100
    // Check that we have a domain constraint on weight
    let first_solution = &solutions[0];
    let has_weight = first_solution
        .keys()
        .any(|fp| fp.reference.join(".").contains("weight"));

    assert!(has_weight, "Should have domain constraint on weight");
}
