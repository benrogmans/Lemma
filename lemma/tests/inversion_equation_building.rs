//! Test cases for verifying constraint equation building
//!
//! These tests verify that:
//! 1. Constraint equations are built correctly from rule branches
//! 2. Rule references are substituted with their equations
//! 3. The equation structure matches the plan

use lemma::{Engine, FactPath, LiteralValue, OperationResult};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn test_simple_equation_building_no_rule_refs() {
    // Simple rule with no rule references
    // rule target = 0
    //   unless condition1 then 1
    //   unless condition2 then 2
    //
    // Expected equation: (NOT condition1 AND NOT condition2 AND result == target) OR
    //                    (condition1 AND NOT condition2 AND result == target) OR
    //                    (condition2 AND result == target)
    let code = r#"
        doc test
        fact x = [boolean]
        fact y = [boolean]
        
        rule target = 0
          unless x then 1
          unless y then 2
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Test with any_value target - should get all 3 branches
    let response = engine
        .invert_strict("test", "target", "=", None, HashMap::new())
        .expect("invert should succeed");

    // Should have 3 solutions: default (0), x=true (1), y=true (2)
    assert_eq!(
        response.solutions.len(),
        3,
        "Should have 3 solutions for 3 branches"
    );
}

#[test]
fn test_equation_with_rule_reference() {
    // Rule that references another rule in condition
    // rule tier = "bronze" unless points >= 100 then "silver"
    // rule rate = 5% unless tier? == "silver" then 10%
    //
    // When inverting rate for 10%:
    // - rate equation: (NOT(tier? == "silver") AND result == 5%) OR (tier? == "silver" AND result == 10%)
    // - tier equation: (NOT(points >= 100) AND result == "bronze") OR (points >= 100 AND result == "silver")
    // - Substitute tier? == "silver" with tier equation filtered for "silver"
    // - Result: (points >= 100 AND result == 10%)
    let code = r#"
        doc test
        fact points = [number]
        
        rule tier = "bronze"
          unless points >= 100 then "silver"
        
        rule rate = 5%
          unless tier? == "silver" then 10%
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "test",
            "rate",
            "=",
            Some(OperationResult::Value(LiteralValue::Percentage(Decimal::from(10)))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "Should have 1 solution for 10% rate"
    );

    // Should have points domain constraint: points >= 100
    let points_path = FactPath::local("points".to_string());
    let points_constraint = response
        .solutions
        .first()
        .and_then(|s| s.fact_constraints.get(&points_path));

    assert!(
        points_constraint.is_some(),
        "Should have points domain constraint"
    );
}

#[test]
fn test_equation_building_preserves_branch_structure() {
    // Verify that the equation structure correctly represents all branches
    let code = r#"
        doc test
        fact discount_code = [text]
        fact member_level = [text]
        
        rule target = 0
          unless discount_code == "SAVE30" and member_level == "platinum" then 1
          unless not (discount_code == "SAVE30") and member_level == "gold" then 2
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("test", "target", "=", None, HashMap::new())
        .expect("invert should succeed");

    // Should have 3 solutions: default (0), branch1 (1), branch2 (2)
    assert_eq!(
        response.solutions.len(),
        3,
        "Should have 3 solutions for 3 branches (default + 2 unless)"
    );

    // Verify outcomes
    let outcomes: Vec<String> = response
        .solutions
        .iter()
        .map(|s| s.outcome.to_string())
        .collect();

    assert!(outcomes.contains(&"0".to_string()), "Should have outcome 0");
    assert!(outcomes.contains(&"1".to_string()), "Should have outcome 1");
    assert!(outcomes.contains(&"2".to_string()), "Should have outcome 2");
}
