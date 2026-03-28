use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, LiteralValue, Target};
use std::collections::HashMap;

fn setup_engine(code: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test"))
        .expect("Failed to add code");
    engine
}

#[test]
fn test_inversion_simple_arithmetic() {
    let code = r#"
        spec pricing
        fact price: [number]
        fact quantity: [number]
        rule total: price * quantity
    "#;
    let engine = setup_engine(code);
    let now = DateTimeValue::now();

    // Invert: total = 100 with no facts provided
    // Should return a solution with shape (price * quantity) = 100
    let result = engine.invert(
        "pricing",
        &now,
        "total",
        Target::value(LiteralValue::number(100.into())),
        HashMap::new(),
    );
    assert!(result.is_ok(), "Inversion should succeed: {:?}", result);

    let response = result.unwrap();

    // Should have at least one solution
    assert!(!response.is_empty(), "Should have at least one solution");

    // When there are multiple unknowns, the solution should have a shape
    let first_solution = &response.solutions[0];
    assert!(
        first_solution.shape.is_some(),
        "With multiple unknowns, solution should have a shape representing the constraint"
    );
}

#[test]
fn test_inversion_veto_query() {
    let code = r#"
        spec shipping
        fact weight: [number]
        rule shipping_cost: 5
          unless weight >= 10 then 10
          unless weight >= 50 then 25
          unless weight < 0 then veto "invalid"
          unless weight > 100 then veto "too heavy"
    "#;
    let engine = setup_engine(code);
    let now = DateTimeValue::now();

    // Query for "too heavy" veto
    let result = engine.invert(
        "shipping",
        &now,
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
    let first_domains = &response.domains[0];
    let has_weight = first_domains.keys().any(|fp| fp.fact.contains("weight"));

    assert!(has_weight, "Should have domain constraint on weight");
}
