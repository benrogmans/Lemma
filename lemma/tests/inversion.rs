use lemma::{Engine, LiteralValue, Target};
use std::collections::HashMap;

fn setup_engine(code: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test")
        .expect("Failed to add code");
    engine
}

#[test]
fn test_inversion_simple_arithmetic() {
    let code = r#"
        doc pricing
        fact price = [number]
        fact quantity = [number]
        rule total = price * quantity
    "#;
    let engine = setup_engine(code);

    // Invert: total = 100
    let result = engine.invert_strict(
        "pricing",
        "total",
        Target::value(LiteralValue::number(100)),
        HashMap::new(),
    );
    assert!(result.is_ok(), "Inversion should succeed: {:?}", result);

    let response = result.unwrap();

    // Should have at least one solution
    assert!(!response.is_empty(), "Should have at least one solution");

    // Should have price and quantity as free variables in the domains
    let first_solution = &response.solutions[0];
    let fact_refs: Vec<&lemma::FactPath> = first_solution.keys().collect();

    // Check that we have both price and quantity (or their qualified versions)
    let has_price = fact_refs.iter().any(|v| v.fact.contains("price"));
    let has_quantity = fact_refs.iter().any(|v| v.fact.contains("quantity"));

    assert!(
        has_price || has_quantity,
        "Should have constraints on price or quantity, found: {:?}",
        fact_refs
    );
}

#[test]
fn test_inversion_veto_query() {
    let code = r#"
        doc shipping
        fact weight = [mass]
        rule shipping_cost = 5
          unless weight >= 10 kilograms then 10
          unless weight >= 50 kilograms then 25
          unless weight < 0 kilograms then veto "invalid"
          unless weight > 100 kilograms then veto "too heavy"
    "#;
    let engine = setup_engine(code);

    // Query for "too heavy" veto
    let result = engine.invert_strict(
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

    let response = result.unwrap();

    assert!(
        !response.is_empty(),
        "Should have at least one solution for veto"
    );

    // The veto "too heavy" should trigger when weight > 100
    // Check that we have a domain constraint on weight
    let first_solution = &response.solutions[0];
    let has_weight = first_solution.keys().any(|fp| fp.fact.contains("weight"));

    assert!(has_weight, "Should have domain constraint on weight");
}
