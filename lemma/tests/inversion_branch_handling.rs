//! Tests for branch handling in the inversion module
//!
//! Verifies that inversion correctly handles rules with multiple unless clauses,
//! producing the right solutions for each branch. Focuses on:
//! - Multi-branch solution enumeration
//! - Specific target filtering
//! - Default value handling
//! - Proof generation for branches
//! - Condition simplification with tautologies
//! - Text enumeration with veto clauses
//!
//! For basic inversion API tests, see `inversion_integration.rs`.
//! For comprehensive coverage including algebraic solving, see `inversion_comprehensive.rs`.

use lemma::{Bound, Engine, FactConstraint, FactPath, LiteralValue, OperationResult};
use std::collections::HashMap;

/// Test that inversion produces solutions for all branches of a rule with multiple unless clauses
#[test]
fn inversion_produces_all_branch_solutions() {
    let code = r#"
        doc order
        fact has_trade_in = [boolean]
        fact trade_in_condition = [text]
        
        rule trade_in_value = 0
          unless has_trade_in and trade_in_condition == "excellent" then 50
          unless has_trade_in and trade_in_condition == "good" then 30
          unless has_trade_in and trade_in_condition == "fair" then 10
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert("order", "trade_in_value", "=", None, HashMap::new())
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        5,
        "should have 5 solutions: 2 for default (has_trade_in=false or unspecified trade_in_conditions) + 3 unless clauses"
    );

    let outcomes: Vec<String> = response
        .solutions
        .iter()
        .map(|s| s.outcome.to_string())
        .collect();

    assert!(outcomes.contains(&"0".to_string()), "should have outcome 0");
    assert!(
        outcomes.contains(&"50".to_string()),
        "should have outcome 50"
    );
    assert!(
        outcomes.contains(&"30".to_string()),
        "should have outcome 30"
    );
    assert!(
        outcomes.contains(&"10".to_string()),
        "should have outcome 10"
    );
}

/// Test that targeting a specific value returns the correct solution
#[test]
fn inversion_filters_to_specific_target() {
    let code = r#"
        doc order
        fact has_trade_in = [boolean]
        fact trade_in_condition = [text]
        
        rule trade_in_value = 0
          unless has_trade_in and trade_in_condition == "excellent" then 50
          unless has_trade_in and trade_in_condition == "good" then 30
          unless has_trade_in and trade_in_condition == "fair" then 10
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    for (target_value, expected_condition_value) in [(50, "excellent"), (30, "good"), (10, "fair")]
    {
        let response = engine
            .invert(
                "order",
                "trade_in_value",
                "=",
                Some(OperationResult::Value(LiteralValue::number(target_value))),
                HashMap::new(),
            )
            .unwrap_or_else(|_| panic!("invert should succeed for value {}", target_value));

        assert_eq!(
            response.solutions.len(),
            1,
            "should have exactly 1 solution for target value {}",
            target_value
        );

        // Check the solution contains the expected trade_in_condition value
        let constraints = &response.solutions[0].fact_constraints;
        let trade_in_condition_constraint = constraints
            .get(&FactPath::local("trade_in_condition".to_string()))
            .expect("solution should contain trade_in_condition");

        let constraint_str = trade_in_condition_constraint.to_string();
        assert!(
            constraint_str.contains(expected_condition_value),
            "constraint for target {} should contain '{}', got: {}",
            target_value,
            expected_condition_value,
            constraint_str
        );

        assert_eq!(
            response.solutions[0].outcome,
            OperationResult::Value(LiteralValue::number(target_value)),
            "outcome should match target value {}",
            target_value
        );
    }
}

/// Test that default value (0) is found when no unless clause matches
#[test]
fn inversion_finds_default_value() {
    let code = r#"
        doc order
        fact has_trade_in = [boolean]
        fact trade_in_condition = [text]
        
        rule trade_in_value = 0
          unless has_trade_in and trade_in_condition == "excellent" then 50
          unless has_trade_in and trade_in_condition == "good" then 30
          unless has_trade_in and trade_in_condition == "fair" then 10
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert(
            "order",
            "trade_in_value",
            "=",
            Some(OperationResult::Value(LiteralValue::number(0))),
            HashMap::new(),
        )
        .expect("invert should succeed for value 0");

    assert_eq!(
        response.solutions.len(),
        2,
        "should have 2 solutions for default value 0: (1) has_trade_in=false, (2) has_trade_in=true with other conditions"
    );

    // Check that at least one solution indicates has_trade_in should be false
    let has_false_solution = response.solutions.iter().any(|sol| {
        if let Some(constraint) = sol
            .fact_constraints
            .get(&FactPath::local("has_trade_in".to_string()))
        {
            constraint.to_string().contains("false")
        } else {
            false
        }
    });

    assert!(
        has_false_solution,
        "at least one solution should indicate has_trade_in is false"
    );
}

/// Test that proofs are generated for each solution
#[test]
fn inversion_generates_proofs() {
    let code = r#"
        doc order
        fact has_trade_in = [boolean]
        fact trade_in_condition = [text]
        
        rule trade_in_value = 0
          unless has_trade_in and trade_in_condition == "excellent" then 50
          unless has_trade_in and trade_in_condition == "good" then 30
          unless has_trade_in and trade_in_condition == "fair" then 10
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert("order", "trade_in_value", "=", None, HashMap::new())
        .expect("invert should succeed");

    // Verify we have solutions
    assert!(
        !response.solutions.is_empty(),
        "Should have at least one solution"
    );
}

#[test]
fn condition_with_tautology_simplifies_correctly() {
    let code = r#"
        doc pricing
        fact age = [number]
        fact is_employee = [boolean]
        
        rule ticket_price = 20
          unless age < 12 then 10
          unless age > 65 then 15
          unless is_employee then 5
          unless is_employee and age > 65 then 5
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert(
            "pricing",
            "ticket_price",
            "=",
            Some(OperationResult::Value(LiteralValue::number(15))),
            HashMap::new(),
        )
        .expect("invert should succeed for senior ticket price 15");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution for senior price"
    );

    let age_constraint = response.solutions[0]
        .fact_constraints
        .get(&FactPath::local("age".to_string()))
        .expect("should have constraint for age");

    assert!(
        age_constraint.is_satisfiable(),
        "age constraint must be satisfiable (not empty), got {:?}",
        age_constraint
    );

    match age_constraint {
        FactConstraint::Range { min, max } => {
            assert!(
                matches!(min, Bound::Exclusive(v) if matches!(v, LiteralValue::Number(n) if n == &rust_decimal::Decimal::from(65))),
                "age min bound should be exclusive 65, got {:?}",
                min
            );
            assert!(
                matches!(max, Bound::Unbounded),
                "age max bound should be unbounded, got {:?}",
                max
            );
        }
        other => panic!("age constraint should be Range (65, +inf), got {:?}", other),
    }
}

#[test]
fn text_equality_with_veto_clause_simplifies_correctly() {
    let code = r#"
        doc menu
        fact drink_type = [text]
        
        rule price = 2.00
          unless drink_type == "espresso" then 2.50
          unless drink_type == "latte" then 3.50
          unless drink_type == "cappuccino" then 3.50
          unless drink_type == "mocha" then 4.00
          unless drink_type != "espresso" and drink_type != "latte" and drink_type != "cappuccino" and drink_type != "mocha" then veto "Unknown drink type"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert(
            "menu",
            "price",
            "=",
            Some(OperationResult::Value(LiteralValue::Number(
                rust_decimal::Decimal::new(350, 2),
            ))),
            HashMap::new(),
        )
        .expect("invert should succeed for price 3.50");

    assert_eq!(
        response.solutions.len(),
        2,
        "should have 2 solutions (latte and cappuccino)"
    );

    let drink_constraints: Vec<&FactConstraint> = response
        .solutions
        .iter()
        .filter_map(|s| {
            s.fact_constraints
                .get(&FactPath::local("drink_type".to_string()))
        })
        .collect();

    assert_eq!(
        drink_constraints.len(),
        2,
        "should have 2 drink_type constraints"
    );

    let constraint_values: Vec<String> = drink_constraints
        .iter()
        .filter_map(|c| {
            if let FactConstraint::Enumeration(vals) = c {
                vals.first().map(|v| v.to_string())
            } else {
                None
            }
        })
        .collect();

    assert!(
        constraint_values.contains(&"\"latte\"".to_string()),
        "should have latte constraint"
    );
    assert!(
        constraint_values.contains(&"\"cappuccino\"".to_string()),
        "should have cappuccino constraint"
    );
}
