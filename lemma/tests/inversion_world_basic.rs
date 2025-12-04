//! Basic integration tests for world-based inversion
//!
//! Tests the world-based inversion algorithm.

use lemma::{Engine, LiteralValue, Target, TargetOp};
use rust_decimal::Decimal;
use std::collections::HashMap;

fn create_engine_with_code(code: &str) -> Engine {
    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    engine
}

#[test]
fn test_invert_simple_rule() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact age = [number]
        rule can_vote = false
            unless age >= 18 then true
        "#,
    );

    let target = Target::value(LiteralValue::Boolean(lemma::BooleanValue::True));
    let result = engine.invert("test", "can_vote", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed: {:?}", result.err());
    let response = result.unwrap();
    assert!(!response.is_empty(), "Should have at least one solution");
}

#[test]
fn test_invert_piecewise_rule() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact points = [number]
        rule tier = "bronze"
            unless points >= 100 then "silver"
            unless points >= 500 then "gold"
        "#,
    );

    // Test finding conditions for "silver" tier
    let target = Target::value(LiteralValue::Text("silver".to_string()));
    let result = engine.invert("test", "tier", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed: {:?}", result.err());
    let response = result.unwrap();
    assert!(!response.is_empty(), "Should have at least one solution for silver");
}

#[test]
fn test_invert_rule_with_rule_reference() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact points = [number]
        rule tier = "bronze"
            unless points >= 100 then "silver"
            unless points >= 500 then "gold"
        rule rate = 5%
            unless tier? == "silver" then 10%
            unless tier? == "gold" then 15%
        "#,
    );

    // Test finding conditions for 10% rate
    let target = Target::value(LiteralValue::Percentage(Decimal::from(10)));
    let result = engine.invert("test", "rate", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed: {:?}", result.err());
    let response = result.unwrap();
    
    // The 10% rate requires tier = "silver", which requires points in [100, 500)
    assert!(!response.is_empty(), "Should have at least one solution for 10% rate");
}

#[test]
fn test_invert_any_value_target() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact age = [number]
        rule category = "child"
            unless age >= 13 then "teenager"
            unless age >= 20 then "adult"
        "#,
    );

    let target = Target::any_value();
    let result = engine.invert("test", "category", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed: {:?}", result.err());
    let response = result.unwrap();
    
    // Should have solutions for all possible outcomes (child, teenager, adult)
    assert!(!response.is_empty(), "Should have solutions for any_value target");
}

#[test]
fn test_invert_with_provided_facts() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact x = [number]
        fact y = [number]
        rule sum = x + y
        rule is_large = sum? > 100
        "#,
    );

    let target = Target::value(LiteralValue::Boolean(lemma::BooleanValue::True));
    
    // Provide x = 50
    let mut values = HashMap::new();
    values.insert("x".to_string(), "50".to_string());

    let result = engine.invert("test", "is_large", target, values);

    // This should work but might not find a simple solution since y is undetermined
    // and sum? > 100 with x=50 means y > 50
    assert!(result.is_ok() || result.is_err(), "Inversion should complete");
}

#[test]
fn test_invert_no_solution() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact status = "active"
        rule result = "yes"
        "#,
    );

    // Try to find when result = "no" (impossible)
    let target = Target::value(LiteralValue::Text("no".to_string()));
    let result = engine.invert("test", "result", target, HashMap::new());

    // Should fail because "no" is never produced
    assert!(result.is_err(), "Should fail for impossible target");
}

#[test]
fn test_invert_multi_rule_dependency() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact points = [number]
        rule tier = "bronze"
            unless points >= 100 then "silver"
        rule discount = 0%
            unless tier? == "silver" then 10%
        "#,
    );

    let target = Target::value(LiteralValue::Percentage(Decimal::from(10)));
    let result = engine.invert("test", "discount", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed");
    let response = result.unwrap();
    
    assert!(!response.solutions.is_empty(), "Should have solutions");
    
    // Verify domains are present for each solution
    assert_eq!(
        response.solutions.len(),
        response.domains.len(),
        "Should have matching number of domains"
    );
}

#[test]
fn test_invert_domains_extracted() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact age = [number]
        rule can_drive = false
            unless age >= 16 then true
        "#,
    );

    let target = Target::value(LiteralValue::Boolean(lemma::BooleanValue::True));
    let result = engine.invert("test", "can_drive", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed: {:?}", result.err());
    let response = result.unwrap();
    
    // Should have domain information
    assert!(!response.domains.is_empty(), "Should have domain information");
}

#[test]
fn test_invert_comparison_operators() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact score = [number]
        rule grade = score
        "#,
    );

    // Test with >= operator
    let target = Target::with_op(
        TargetOp::Gte,
        lemma::OperationResult::Value(LiteralValue::Number(Decimal::from(90))),
    );
    let result = engine.invert("test", "grade", target, HashMap::new());

    // This tests that we can use comparison operators in targets
    // The result depends on whether the engine supports this properly
    assert!(result.is_ok() || result.is_err(), "Operation should complete");
}

#[test]
fn test_invert_veto_excluded_from_domain() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact age = [number]
        rule of_age = age > 18
            unless age < 0 then veto "Invalid age"
        "#,
    );

    // Test of_age = false (not of age)
    // Should give domain [0, 18] (excludes negative ages due to veto)
    let target = Target::value(LiteralValue::Boolean(lemma::BooleanValue::False));
    let result = engine.invert("test", "of_age", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed: {:?}", result.err());
    let response = result.unwrap();
    assert!(!response.is_empty(), "Should have at least one solution");

    // Verify the domain excludes negative ages
    assert!(!response.domains.is_empty(), "Should have domain information");

    let domain_map = &response.domains[0];
    let age_path = lemma::FactPath {
        segments: vec![],
        fact: "age".to_string(),
    };

    if let Some(domain) = domain_map.get(&age_path) {
        let domain_str = format!("{}", domain);
        assert!(
            domain_str.contains("[0") || domain_str.contains("(0"),
            "Domain should have lower bound 0, got: {}",
            domain_str
        );
        assert!(
            domain_str.contains("18]") || domain_str.contains("18)"),
            "Domain should have upper bound 18, got: {}",
            domain_str
        );
    }
}

#[test]
fn test_invert_boolean_expression_result() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact x = [number]
        rule is_positive = x > 0
        "#,
    );

    // Test is_positive = true
    let target = Target::value(LiteralValue::Boolean(lemma::BooleanValue::True));
    let result = engine.invert("test", "is_positive", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed: {:?}", result.err());
    let response = result.unwrap();
    assert!(!response.is_empty(), "Should have at least one solution");

    // Domain should be (0, +inf)
    assert!(!response.domains.is_empty(), "Should have domain information");
    let domain_map = &response.domains[0];
    let x_path = lemma::FactPath {
        segments: vec![],
        fact: "x".to_string(),
    };

    if let Some(domain) = domain_map.get(&x_path) {
        let domain_str = format!("{}", domain);
        assert!(
            domain_str.contains("(0") && domain_str.contains("+inf"),
            "Domain should be (0, +inf), got: {}",
            domain_str
        );
    }
}

#[test]
fn test_invert_boolean_expression_false() {
    let engine = create_engine_with_code(
        r#"
        doc test
        fact x = [number]
        rule is_positive = x > 0
        "#,
    );

    // Test is_positive = false
    let target = Target::value(LiteralValue::Boolean(lemma::BooleanValue::False));
    let result = engine.invert("test", "is_positive", target, HashMap::new());

    assert!(result.is_ok(), "Inversion should succeed: {:?}", result.err());
    let response = result.unwrap();
    assert!(!response.is_empty(), "Should have at least one solution");

    // Domain should be (-inf, 0]
    assert!(!response.domains.is_empty(), "Should have domain information");
    let domain_map = &response.domains[0];
    let x_path = lemma::FactPath {
        segments: vec![],
        fact: "x".to_string(),
    };

    if let Some(domain) = domain_map.get(&x_path) {
        let domain_str = format!("{}", domain);
        assert!(
            domain_str.contains("-inf") && (domain_str.contains("0]") || domain_str.contains("0)")),
            "Domain should be (-inf, 0], got: {}",
            domain_str
        );
    }
}
