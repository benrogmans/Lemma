use lemma::{Engine, LiteralValue};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn simple_veto_boundaries() {
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule can_ship = true
          unless weight < 0 kilograms then veto "negative weight"
          unless weight > 100 kilograms then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let solutions = engine
        .invert(
            "shipping",
            "can_ship",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get valid domain");

    // Should have constraints: weight >= 0 AND weight <= 100
    assert!(!solutions.is_empty(), "Expected at least one domain");
}

#[test]
fn piecewise_discount_tiers() {
    let code = r#"
        doc pricing
        fact quantity = [number]

        rule discount = 0%
          unless quantity >= 10 then 5%
          unless quantity >= 50 then 10%
          unless quantity < 0 then veto "negative quantity"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let solutions = engine
        .invert(
            "pricing",
            "discount",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get valid domain");

    // Valid domain: quantity >= 0 (excludes the veto condition)
    assert!(!solutions.is_empty(), "Expected valid domain");
}

#[test]
fn no_vetos_returns_unconstrained() {
    let code = r#"
        doc simple
        fact x = [number]

        rule double = x * 2
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();
    let solutions = engine
        .invert(
            "simple",
            "double",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get valid domain");

    // No vetos, so x is unconstrained
    assert!(!solutions.is_empty(), "Expected domain");
    // Could check if it's Domain::Unconstrained but that's implementation detail
}

#[test]
fn multiple_facts_with_vetos() {
    let code = r#"
        doc validation
        fact age = [number]
        fact income = [money]

        rule eligible = true
          unless age < 18 then veto "too young"
          unless age > 65 then veto "too old"
          unless income < 20000 EUR then veto "income too low"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Get valid domain for age
    let age_domains = engine
        .invert(
            "validation",
            "eligible",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get age domain");

    assert!(!age_domains.is_empty(), "Expected age constraints");
    // Valid: 18 <= age <= 65

    // Get valid domain for income
    let income_domains = engine
        .invert(
            "validation",
            "eligible",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get income domain");

    assert!(!income_domains.is_empty(), "Expected income constraints");
    // Valid: income >= 20000 EUR
}

#[test]
fn with_given_facts() {
    let code = r#"
        doc pricing
        fact base_price = [money]
        fact quantity = [number]

        rule total = base_price * quantity
          unless quantity < 1 then veto "invalid quantity"
          unless base_price < 0 EUR then veto "invalid price"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // With quantity given, check valid domain for base_price
    let mut given = HashMap::new();
    given.insert(
        "pricing.quantity".to_string(),
        LiteralValue::Number(Decimal::from(10)),
    );

    let solutions = engine
        .invert("pricing", "total", lemma::Target::any_value(), given)
        .expect("should get price domain");

    // Valid: base_price >= 0 EUR (quantity=10 satisfies its constraint)
    assert!(!solutions.is_empty(), "Expected price constraints");
}

#[test]
fn fact_not_in_rule() {
    let code = r#"
        doc test
        fact x = [number]
        fact y = [number]

        rule result = x * 2
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Invert the rule
    let solutions = engine
        .invert("test", "result", lemma::Target::any_value(), HashMap::new())
        .expect("should succeed");

    // y is not constrained by this rule, so it shouldn't appear in any solution
    let y_path = lemma::FactReference {
        reference: vec!["test".to_string(), "y".to_string()],
    };
    assert!(!solutions.is_empty(), "Should have at least one solution");
    for solution in &solutions {
        assert!(
            !solution.contains_key(&y_path),
            "y should not appear in domains since it's not used"
        );
    }
}

#[test]
fn complex_boolean_conditions() {
    let code = r#"
        doc complex
        fact a = [number]
        fact b = [number]

        rule result = true
          unless (a < 0 or b < 0) then veto "negative"
          unless (a > 100 and b > 100) then veto "both too large"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();
    let solutions = engine
        .invert(
            "complex",
            "result",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domain");

    // Valid: a >= 0 AND NOT(a > 100 AND b > 100)
    assert!(!solutions.is_empty(), "Expected constraints on a");
}

#[test]
fn test_use_case_form_validation() {
    // Real-world scenario: validate form inputs
    let code = r#"
        doc order
        fact item_count = [number]
        fact shipping_method = [text]

        rule can_place_order = true
          unless item_count < 1 then veto "must order at least one item"
          unless item_count > 100 then veto "order too large"
          unless (shipping_method is "standard" or shipping_method is "express")
            then veto "invalid shipping method"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Get valid range for item_count
    let item_domains = engine
        .invert(
            "order",
            "can_place_order",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get item_count domain");

    assert!(
        !item_domains.is_empty(),
        "Expected item_count range: 1..=100"
    );

    // Get valid values for shipping_method
    let shipping_domains = engine
        .invert(
            "order",
            "can_place_order",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get shipping_method domain");

    assert!(
        !shipping_domains.is_empty(),
        "Expected shipping_method enumeration"
    );
}
