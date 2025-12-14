//! Comprehensive tests for the inversion module
//!
//! This is the main test suite for inversion functionality. Tests cover:
//! - Boolean fact combinations
//! - Text enumerations
//! - Veto handling (any_value target)
//! - Complex multi-rule scenarios with literal outcomes
//! - Rule references expansion
//! - Proof generation
//! - Algebraic solving (solving equations like `price * 5 = 50` for price)
//!
//! For basic inversion API tests, see `inversion_integration.rs`.
//! For detailed branch handling scenarios, see `inversion_branch_handling.rs`.

use lemma::{BooleanValue, Bound, Engine, FactConstraint, FactPath, LiteralValue, OperationResult};
use rust_decimal::Decimal;
use std::collections::HashMap;

// =============================================================================
// BOOLEAN FACT COMBINATIONS
// =============================================================================

#[test]
fn boolean_multiple_unless_clauses_specific_target() {
    let code = r#"
        doc shop
        fact is_member = [boolean]
        fact is_premium = [boolean]
        fact has_coupon = [boolean]
        
        rule discount = 0%
          unless is_member then 10%
          unless is_premium then 20%
          unless has_coupon then 5%
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "shop",
            "discount",
            "=",
            Some(OperationResult::Value(LiteralValue::Percentage(
                Decimal::from(10),
            ))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution for 10%"
    );

    let solution = &response.solutions[0];
    assert_eq!(
        solution.outcome,
        OperationResult::Value(LiteralValue::Percentage(Decimal::from(10))),
        "outcome should be 10%"
    );

    // Verify the solution contains is_member constraint
    assert!(
        solution
            .fact_constraints
            .contains_key(&FactPath::local("is_member".to_string())),
        "solution should contain is_member constraint"
    );
}

#[test]
fn boolean_multiple_unless_clauses_any_value() {
    let code = r#"
        doc shop
        fact is_member = [boolean]
        fact is_premium = [boolean]
        fact has_coupon = [boolean]
        
        rule discount = 0%
          unless is_member then 10%
          unless is_premium then 20%
          unless has_coupon then 5%
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("shop", "discount", "=", None, HashMap::new())
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        4,
        "should have 4 solutions (0%, 10%, 20%, 5%)"
    );

    let outcomes: Vec<String> = response
        .solutions
        .iter()
        .map(|s| s.outcome.to_string())
        .collect();

    assert!(
        outcomes.iter().any(|o| o.contains("0")),
        "should have 0% outcome"
    );
    assert!(
        outcomes.iter().any(|o| o.contains("10")),
        "should have 10% outcome"
    );
    assert!(
        outcomes.iter().any(|o| o.contains("20")),
        "should have 20% outcome"
    );
    assert!(
        outcomes.iter().any(|o| o.contains("5")),
        "should have 5% outcome"
    );
}

// =============================================================================
// TEXT ENUMERATION AND VETO HANDLING
// =============================================================================

#[test]
fn text_enumeration_with_veto() {
    let code = r#"
        doc workflow
        fact status = [text]
        
        rule can_proceed = false
          unless status == "approved" then true
          unless status == "pending" then veto "awaiting review"
          unless status == "rejected" then veto "application rejected"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "workflow",
            "can_proceed",
            "=",
            Some(OperationResult::Value(LiteralValue::Boolean(
                BooleanValue::True,
            ))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution"
    );

    let status_constraint = response.solutions.first().and_then(|s| {
        s.fact_constraints
            .get(&FactPath::local("status".to_string()))
    });

    assert_eq!(
        status_constraint,
        Some(&FactConstraint::Enumeration(vec![LiteralValue::Text(
            "approved".to_string()
        )])),
        "status should be exactly 'approved'"
    );
}

#[test]
fn any_value_includes_veto_outcomes() {
    let code = r#"
        doc workflow
        fact status = [text]
        
        rule can_proceed = false
          unless status == "approved" then true
          unless status == "pending" then veto "awaiting review"
          unless status == "rejected" then veto "application rejected"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("workflow", "can_proceed", "=", None, HashMap::new())
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        4,
        "should have 4 solutions (false, true, 2 vetos)"
    );

    let has_veto = response
        .solutions
        .iter()
        .any(|s| s.outcome.to_string().contains("veto"));

    assert!(
        has_veto,
        "should include veto outcomes when using any_value"
    );
}

#[test]
fn veto_boundary_produces_range() {
    let code = r#"
        doc shipping
        fact weight = [mass]
        
        rule can_ship = true
          unless weight > 100 kilograms then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("shipping", "can_ship", "=", None, HashMap::new())
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        2,
        "should have 2 solutions (true, veto)"
    );

    let true_solution = response
        .solutions
        .iter()
        .find(|s| s.outcome == OperationResult::Value(LiteralValue::Boolean(BooleanValue::True)));
    assert!(true_solution.is_some(), "should have a true solution");

    let true_idx = response
        .solutions
        .iter()
        .position(|s| {
            s.outcome == OperationResult::Value(LiteralValue::Boolean(BooleanValue::True))
        })
        .unwrap();

    let weight_constraint = response.solutions.get(true_idx).and_then(|s| {
        s.fact_constraints
            .get(&FactPath::local("weight".to_string()))
    });

    assert!(
        weight_constraint.is_some(),
        "should have weight constraint for true solution"
    );
    let weight_constraint = weight_constraint.unwrap();

    match weight_constraint {
        FactConstraint::Range { min, max } => {
            assert!(matches!(min, Bound::Unbounded), "min should be unbounded");
            match max {
                Bound::Inclusive(v) => {
                    let is_100 = match v {
                        LiteralValue::Number(n) => n == &Decimal::from(100),
                        LiteralValue::Unit(u) => u.value() == Decimal::from(100),
                        _ => false,
                    };
                    assert!(is_100, "max should be 100");
                }
                _ => panic!("max should be Inclusive(100)"),
            }
        }
        FactConstraint::Complement(inner) => match inner.as_ref() {
            FactConstraint::Range { min, .. } => match min {
                Bound::Exclusive(v) => {
                    let is_100 = match v {
                        LiteralValue::Number(n) => n == &Decimal::from(100),
                        LiteralValue::Unit(u) => u.value() == Decimal::from(100),
                        _ => false,
                    };
                    assert!(is_100, "complement range min should be 100");
                }
                _ => panic!("should have exclusive bound at 100"),
            },
            _ => panic!("complement should contain a range"),
        },
        _ => panic!(
            "weight should be Range or Complement, got {:?}",
            weight_constraint
        ),
    }
}

// =============================================================================
// RULE REFERENCE EXPANSION
// =============================================================================

#[test]
fn rule_references_expand_correctly() {
    let code = r#"
        doc rewards
        fact points = [number]
        fact base_amount = [number]
        
        rule tier = "bronze"
          unless points >= 100 then "silver"
          unless points >= 500 then "gold"
          unless points >= 1000 then "platinum"
        
        rule rate = 5%
          unless tier? == "silver" then 10%
          unless tier? == "gold" then 15%
          unless tier? == "platinum" then 20%
        
        rule final_amount = base_amount * rate?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "rewards",
            "rate",
            "=",
            Some(OperationResult::Value(LiteralValue::Percentage(
                Decimal::from(15),
            ))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution for 15%"
    );

    let points_domain = response.solutions.first().and_then(|s| {
        s.fact_constraints
            .get(&FactPath::local("points".to_string()))
    });

    assert!(points_domain.is_some(), "should have points domain");

    match points_domain.unwrap() {
        FactConstraint::Range { min, max } => {
            match min {
                Bound::Inclusive(v) => {
                    assert_eq!(*v, LiteralValue::number(500), "min should be 500");
                }
                _ => panic!("min should be Inclusive(500)"),
            }
            match max {
                Bound::Exclusive(v) => {
                    assert_eq!(
                        *v,
                        LiteralValue::number(1000),
                        "max should be 1000 exclusive"
                    );
                }
                _ => panic!("max should be Exclusive(1000)"),
            }
        }
        _ => panic!("points should be a Range [500, 1000)"),
    }
}

// =============================================================================
// COMPLEX SCENARIOS
// =============================================================================

#[test]
fn complex_pricing_with_member_coupon_combo() {
    let code = r#"
        doc pricing
        fact is_member = [boolean]
        fact has_coupon = [boolean]
        fact is_premium = [boolean]
        
        rule discount = 0%
          unless has_coupon and not is_member then 5%
          unless is_member and not has_coupon then 10%
          unless is_member and has_coupon then 20%
          unless is_premium then 25%
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "pricing",
            "discount",
            "=",
            Some(OperationResult::Value(LiteralValue::Percentage(
                Decimal::from(20),
            ))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution for 20%"
    );

    // Verify solution contains both is_member and has_coupon constraints
    let constraints = &response.solutions[0].fact_constraints;
    assert!(
        constraints.contains_key(&FactPath::local("is_member".to_string())),
        "solution should contain is_member"
    );
    assert!(
        constraints.contains_key(&FactPath::local("has_coupon".to_string())),
        "solution should contain has_coupon"
    );
}

#[test]
fn complex_event_booking() {
    let code = r#"
        doc booking
        fact attendee_count = [number]
        fact is_peak_time = [boolean]
        fact is_member = [boolean]
        fact days_in_advance = [number]
        fact is_special_event = [boolean]
        fact venue_size = [text]
        
        rule max_capacity = 50
          unless venue_size == "medium" then 100
          unless venue_size == "large" then 250
          unless venue_size == "large" and is_special_event then 200
        
        rule min_advance_days = 1
          unless is_peak_time and not is_member then 7
          unless is_special_event then 14
          unless is_special_event and is_member then 7
        
        rule can_book = true
          unless attendee_count > max_capacity? then veto "exceeds capacity"
          unless attendee_count < 1 then veto "must have attendees"
          unless days_in_advance < min_advance_days? then veto "insufficient notice"
          unless is_peak_time and is_special_event and not is_member then veto "members only"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("booking", "can_book", "=", None, HashMap::new())
        .expect("invert should succeed");

    assert!(
        !response.solutions.is_empty(),
        "should have valid booking configurations"
    );
}

// =============================================================================
// MULTI-BRANCH HANDLING (from branch_handling tests)
// =============================================================================

#[test]
fn multi_branch_trade_in_values() {
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

    // Should have solutions for all 4 outcomes
    // Note: The default outcome (0) may have multiple logical paths after normalization
    assert!(
        response.solutions.len() >= 4,
        "should have at least 4 solutions (one per outcome)"
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

    // Verify we have exactly 4 distinct outcomes
    let unique_outcomes: std::collections::HashSet<String> = outcomes.into_iter().collect();
    assert_eq!(
        unique_outcomes.len(),
        4,
        "should have exactly 4 distinct outcomes"
    );
}

#[test]
fn multi_branch_specific_target_filtering() {
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
    }
}

// =============================================================================
// ALGEBRAIC SOLVING
// =============================================================================

#[test]
fn algebraic_solve_simple_multiplication() {
    let code = r#"
        doc shop
        fact price = [number]
        fact quantity = [number]
        
        rule total = price * quantity
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut provided_values = HashMap::new();
    provided_values.insert("quantity".to_string(), LiteralValue::number(5));

    let response = engine
        .invert_strict(
            "shop",
            "total",
            "=",
            Some(OperationResult::Value(LiteralValue::number(50))),
            provided_values,
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution"
    );

    let price_domain = response.solutions.first().and_then(|s| {
        s.fact_constraints
            .get(&FactPath::local("price".to_string()))
    });

    assert!(price_domain.is_some(), "should have price domain");

    match price_domain.unwrap() {
        FactConstraint::Enumeration(values) => {
            assert_eq!(values.len(), 1, "should have exactly 1 value");
            assert_eq!(values[0], LiteralValue::number(10), "price should be 10");
        }
        _ => panic!("price domain should be Enumeration, got {:?}", price_domain),
    }
}

#[test]
fn algebraic_solve_simple_addition() {
    let code = r#"
        doc math
        fact x = [number]
        
        rule result = x + 10
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "math",
            "result",
            "=",
            Some(OperationResult::Value(LiteralValue::number(25))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution"
    );

    let x_domain = response
        .solutions
        .first()
        .and_then(|s| s.fact_constraints.get(&FactPath::local("x".to_string())));

    assert!(x_domain.is_some(), "should have x domain");

    match x_domain.unwrap() {
        FactConstraint::Enumeration(values) => {
            assert_eq!(values.len(), 1, "should have exactly 1 value");
            assert_eq!(values[0], LiteralValue::number(15), "x should be 15");
        }
        _ => panic!("x domain should be Enumeration, got {:?}", x_domain),
    }
}

#[test]
fn algebraic_solve_chained_operations() {
    let code = r#"
        doc payroll
        fact hours = [number]
        fact rate = [number]
        fact bonus = [number]
        
        rule gross_pay = hours * rate + bonus
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut provided_values = HashMap::new();
    provided_values.insert("rate".to_string(), LiteralValue::number(25));
    provided_values.insert("bonus".to_string(), LiteralValue::number(100));

    let response = engine
        .invert_strict(
            "payroll",
            "gross_pay",
            "=",
            Some(OperationResult::Value(LiteralValue::number(1100))),
            provided_values,
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution"
    );

    let hours_domain = response.solutions.first().and_then(|s| {
        s.fact_constraints
            .get(&FactPath::local("hours".to_string()))
    });

    assert!(hours_domain.is_some(), "should have hours domain");

    match hours_domain.unwrap() {
        FactConstraint::Enumeration(values) => {
            assert_eq!(values.len(), 1, "should have exactly 1 value");
            assert_eq!(values[0], LiteralValue::number(40), "hours should be 40");
        }
        _ => panic!("hours domain should be Enumeration, got {:?}", hours_domain),
    }
}

#[test]
fn algebraic_solve_division() {
    let code = r#"
        doc recipe
        fact total_servings = [number]
        fact servings_per_batch = [number]
        
        rule batches_needed = total_servings / servings_per_batch
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut provided_values = HashMap::new();
    provided_values.insert("servings_per_batch".to_string(), LiteralValue::number(6));

    let response = engine
        .invert_strict(
            "recipe",
            "batches_needed",
            "=",
            Some(OperationResult::Value(LiteralValue::number(5))),
            provided_values,
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution"
    );

    let total_domain = response.solutions.first().and_then(|s| {
        s.fact_constraints
            .get(&FactPath::local("total_servings".to_string()))
    });

    assert!(total_domain.is_some(), "should have total_servings domain");

    match total_domain.unwrap() {
        FactConstraint::Enumeration(values) => {
            assert_eq!(values.len(), 1, "should have exactly 1 value");
            assert_eq!(
                values[0],
                LiteralValue::number(30),
                "total_servings should be 30"
            );
        }
        _ => panic!(
            "total_servings domain should be Enumeration, got {:?}",
            total_domain
        ),
    }
}

#[test]
fn algebraic_solve_subtraction_from_left() {
    let code = r#"
        doc finance
        fact original_price = [number]
        fact discount = [number]
        
        rule final_price = original_price - discount
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut provided_values = HashMap::new();
    provided_values.insert("discount".to_string(), LiteralValue::number(20));

    let response = engine
        .invert_strict(
            "finance",
            "final_price",
            "=",
            Some(OperationResult::Value(LiteralValue::number(80))),
            provided_values,
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution"
    );

    let original_domain = response.solutions.first().and_then(|s| {
        s.fact_constraints
            .get(&FactPath::local("original_price".to_string()))
    });

    assert!(
        original_domain.is_some(),
        "should have original_price domain"
    );

    match original_domain.unwrap() {
        FactConstraint::Enumeration(values) => {
            assert_eq!(values.len(), 1, "should have exactly 1 value");
            assert_eq!(
                values[0],
                LiteralValue::number(100),
                "original_price should be 100"
            );
        }
        _ => panic!(
            "original_price domain should be Enumeration, got {:?}",
            original_domain
        ),
    }
}

#[test]
fn algebraic_solve_subtraction_from_right() {
    let code = r#"
        doc finance
        fact total = [number]
        fact payment = [number]
        
        rule remaining = total - payment
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut provided_values = HashMap::new();
    provided_values.insert("total".to_string(), LiteralValue::number(100));

    let response = engine
        .invert_strict(
            "finance",
            "remaining",
            "=",
            Some(OperationResult::Value(LiteralValue::number(30))),
            provided_values,
        )
        .expect("invert should succeed");

    assert_eq!(
        response.solutions.len(),
        1,
        "should have exactly 1 solution"
    );

    let payment_domain = response.solutions.first().and_then(|s| {
        s.fact_constraints
            .get(&FactPath::local("payment".to_string()))
    });

    assert!(payment_domain.is_some(), "should have payment domain");

    match payment_domain.unwrap() {
        FactConstraint::Enumeration(values) => {
            assert_eq!(values.len(), 1, "should have exactly 1 value");
            assert_eq!(values[0], LiteralValue::number(70), "payment should be 70");
        }
        _ => panic!(
            "payment domain should be Enumeration, got {:?}",
            payment_domain
        ),
    }
}
