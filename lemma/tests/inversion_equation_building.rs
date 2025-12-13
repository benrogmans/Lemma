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

    println!("\nGot {} solutions:", response.solutions.len());
    for (i, sol) in response.solutions.iter().enumerate() {
        println!("Solution {}: outcome={}, constraints={}", 
            i, sol.outcome, sol.fact_constraints.len());
        for (path, constraint) in &sol.fact_constraints {
            println!("  {}: {}", path, constraint);
        }
    }

    // Multiple logical paths may exist after normalization
    assert!(
        response.solutions.len() >= 1,
        "Should have at least 1 solution for 10% rate"
    );

    // Verify the outcome is correct
    assert!(
        response.solutions.iter().all(|s| matches!(&s.outcome, OperationResult::Value(LiteralValue::Percentage(p)) if *p == Decimal::from(10))),
        "All solutions should have 10% outcome"
    );
    
    let points_path = FactPath::local("points".to_string());
    let has_points_constraint = response
        .solutions
        .iter()
        .any(|s| s.fact_constraints.contains_key(&points_path));

    assert!(
        has_points_constraint,
        "Should have points domain constraint in at least one solution"
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

    // After normalization, default outcome may have multiple logical paths
    let outcomes: Vec<String> = response
        .solutions
        .iter()
        .map(|s| s.outcome.to_string())
        .collect();

    // Verify all 3 distinct outcomes are present
    let unique_outcomes: std::collections::HashSet<String> = outcomes.iter().cloned().collect();
    assert_eq!(
        unique_outcomes.len(),
        3,
        "Should have 3 distinct outcomes (0, 1, 2)"
    );

    assert!(outcomes.contains(&"0".to_string()), "Should have outcome 0");
    assert!(outcomes.contains(&"1".to_string()), "Should have outcome 1");
    assert!(outcomes.contains(&"2".to_string()), "Should have outcome 2");
}
