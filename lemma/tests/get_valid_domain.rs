#![cfg(feature = "inversion")]

use lemma::{Bound, Domain, Engine, FactPath, LiteralValue};
use std::collections::HashMap;

#[test]
fn simple_veto_boundaries_should_produce_bounded_range() {
    // Rule: can_ship = true
    //   unless weight < 0 then veto
    //   unless weight > 100 then veto
    // Target: any valid value
    // Expected: weight in [0, 100]
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule can_ship = true
          unless weight < 0 kilograms then veto "negative weight"
          unless weight > 100 kilograms then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "shipping",
            "can_ship",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get valid domain");

    assert_eq!(response.len(), 1, "should have exactly one solution");

    let solution = &response.solutions[0];
    let weight_path = FactPath::local("weight".to_string());

    let weight_domain = solution
        .get(&weight_path)
        .expect("solution should contain weight");

    // Weight should be in range [0, 100], not Unconstrained
    match weight_domain {
        Domain::Range { min, max } => {
            match min {
                Bound::Inclusive(v) => assert_eq!(*v, LiteralValue::number(0), "min should be 0"),
                other => panic!("min should be Inclusive(0), got {:?}", other),
            }
            match max {
                Bound::Inclusive(v) => {
                    assert_eq!(*v, LiteralValue::number(100), "max should be 100")
                }
                other => panic!("max should be Inclusive(100), got {:?}", other),
            }
        }
        Domain::Unconstrained => {
            panic!("weight should NOT be Unconstrained - vetos constrain it to [0, 100]");
        }
        other => panic!("expected Range [0, 100], got {:?}", other),
    }
}

#[test]
fn piecewise_discount_should_constrain_quantity() {
    // Rule: discount = 0% unless quantity >= 10 then 5% unless quantity >= 50 then 10%
    //       unless quantity < 0 then veto
    // Target: any valid value
    // Expected: quantity >= 0 (the veto excludes negative)
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

    let response = engine
        .invert_strict(
            "pricing",
            "discount",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get valid domain");

    // Should have multiple solutions (one for each discount tier)
    // but all should have quantity >= 0
    assert!(!response.is_empty(), "should have solutions");

    let quantity_path = FactPath::local("quantity".to_string());

    for (i, solution) in response.solutions.iter().enumerate() {
        let quantity_domain = solution
            .get(&quantity_path)
            .expect("each solution should contain quantity");

        // Quantity should NOT be Unconstrained - at minimum it should be >= 0
        assert!(
            !matches!(quantity_domain, Domain::Unconstrained),
            "solution {} has Unconstrained quantity - should be >= 0 due to veto",
            i
        );
    }
}

#[test]
fn no_vetos_returns_unconstrained() {
    // Rule: double = x * 2
    // Target: any value
    // Expected: x is Unconstrained (no vetos to restrict it)
    let code = r#"
        doc simple
        fact x = [number]

        rule double = x * 2
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "simple",
            "double",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get valid domain");

    assert_eq!(response.len(), 1, "should have one solution");

    let solution = &response.solutions[0];
    let x_path = FactPath::local("x".to_string());

    let x_domain = solution.get(&x_path).expect("solution should contain x");

    // x should be Unconstrained since there are no vetos
    assert_eq!(
        *x_domain,
        Domain::Unconstrained,
        "x should be Unconstrained (no vetos), got {:?}",
        x_domain
    );
}

#[test]
fn multiple_facts_with_vetos_should_constrain_both() {
    // Rule: eligible = true
    //   unless age < 18 then veto
    //   unless age > 65 then veto
    //   unless income < 20000 then veto
    // Target: any valid value
    // Expected: age in [18, 65] AND income >= 20000
    let code = r#"
        doc validation
        fact age = [number]
        fact income = [number]

        rule eligible = true
          unless age < 18 then veto "too young"
          unless age > 65 then veto "too old"
          unless income < 20000 then veto "income too low"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "validation",
            "eligible",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domain");

    assert_eq!(response.len(), 1, "should have one solution");

    let solution = &response.solutions[0];
    let age_path = FactPath::local("age".to_string());
    let income_path = FactPath::local("income".to_string());

    // Check age constraint: should be [18, 65]
    let age_domain = solution
        .get(&age_path)
        .expect("solution should contain age");

    match age_domain {
        Domain::Range { min, max } => {
            match min {
                Bound::Inclusive(v) => {
                    assert_eq!(*v, LiteralValue::number(18), "age min should be 18")
                }
                other => panic!("age min should be Inclusive(18), got {:?}", other),
            }
            match max {
                Bound::Inclusive(v) => {
                    assert_eq!(*v, LiteralValue::number(65), "age max should be 65")
                }
                other => panic!("age max should be Inclusive(65), got {:?}", other),
            }
        }
        Domain::Unconstrained => {
            panic!("age should NOT be Unconstrained - vetos constrain it to [18, 65]");
        }
        other => panic!("expected age Range [18, 65], got {:?}", other),
    }

    // Check income constraint: should be >= 20000
    let income_domain = solution
        .get(&income_path)
        .expect("solution should contain income");

    match income_domain {
        Domain::Range { min, max } => {
            match min {
                Bound::Inclusive(v) => assert_eq!(
                    *v,
                    LiteralValue::number(20000),
                    "income min should be 20000"
                ),
                other => panic!("income min should be Inclusive(20000), got {:?}", other),
            }
            assert!(
                matches!(max, Bound::Unbounded),
                "income max should be unbounded"
            );
        }
        Domain::Unconstrained => {
            panic!("income should NOT be Unconstrained - veto constrains it to >= 20000");
        }
        other => panic!("expected income Range [20000, inf), got {:?}", other),
    }
}

#[test]
fn with_given_facts_should_only_constrain_unknowns() {
    // Rule: total = base_price * quantity
    //   unless quantity < 1 then veto
    //   unless base_price < 0 then veto
    // Given: quantity = 10
    // Target: any valid value
    // Expected: base_price >= 0 (quantity is fixed at 10, satisfies its constraint)
    let code = r#"
        doc pricing
        fact base_price = [number]
        fact quantity = [number]

        rule total = base_price * quantity
          unless quantity < 1 then veto "invalid quantity"
          unless base_price < 0 then veto "invalid price"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert("quantity".to_string(), LiteralValue::number(10));

    let response = engine
        .invert_strict("pricing", "total", lemma::Target::any_value(), given)
        .expect("should get price domain");

    assert_eq!(response.len(), 1, "should have one solution");

    let solution = &response.solutions[0];
    let price_path = FactPath::local("base_price".to_string());

    // quantity should NOT be in the solution (it's given)
    let quantity_path = FactPath::local("quantity".to_string());
    assert!(
        !solution.contains_key(&quantity_path),
        "quantity should not be in solution since it's given"
    );

    // base_price should be >= 0
    let price_domain = solution
        .get(&price_path)
        .expect("solution should contain base_price");

    match price_domain {
        Domain::Range { min, max } => {
            match min {
                Bound::Inclusive(v) => {
                    assert_eq!(*v, LiteralValue::number(0), "price min should be 0")
                }
                other => panic!("price min should be Inclusive(0), got {:?}", other),
            }
            assert!(
                matches!(max, Bound::Unbounded),
                "price max should be unbounded"
            );
        }
        Domain::Unconstrained => {
            panic!("base_price should NOT be Unconstrained - veto constrains it to >= 0");
        }
        other => panic!("expected price Range [0, inf), got {:?}", other),
    }
}

#[test]
fn fact_not_in_rule_should_not_appear_in_solution() {
    // Rule: result = x * 2
    // Facts: x, y (y is not used in rule)
    // Target: any value
    // Expected: y should NOT appear in solution, x should be Unconstrained
    let code = r#"
        doc test
        fact x = [number]
        fact y = [number]

        rule result = x * 2
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("test", "result", lemma::Target::any_value(), HashMap::new())
        .expect("should succeed");

    assert_eq!(response.len(), 1, "should have one solution");

    let solution = &response.solutions[0];

    // y should NOT appear (not used in rule)
    let y_local = FactPath::local("y".to_string());
    let y_qualified = FactPath::from_path(vec!["test".to_string(), "y".to_string()]);
    assert!(
        !solution.contains_key(&y_local) && !solution.contains_key(&y_qualified),
        "y should not appear in solution since it's not used in the rule"
    );

    // x SHOULD appear and be Unconstrained (no vetos)
    let x_path = FactPath::local("x".to_string());
    let x_domain = solution.get(&x_path).expect("solution should contain x");

    assert_eq!(
        *x_domain,
        Domain::Unconstrained,
        "x should be Unconstrained (no vetos), got {:?}",
        x_domain
    );
}

#[test]
fn complex_boolean_conditions_should_produce_constraints() {
    // Rule: result = true
    //   unless (a < 0 or b < 0) then veto "negative"
    //   unless (a > 100 and b > 100) then veto "both too large"
    // Target: any valid value
    // Expected: a >= 0 AND b >= 0 AND NOT(a > 100 AND b > 100)
    // This means: both must be non-negative, and at least one must be <= 100
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

    let response = engine
        .invert_strict(
            "complex",
            "result",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domain");

    assert!(!response.is_empty(), "should have solutions");

    let a_path = FactPath::local("a".to_string());
    let b_path = FactPath::local("b".to_string());

    // At least check that a and b are constrained (not Unconstrained)
    // due to the (a < 0 or b < 0) veto
    for (i, solution) in response.solutions.iter().enumerate() {
        let a_domain = solution.get(&a_path).expect("solution should contain a");
        let b_domain = solution.get(&b_path).expect("solution should contain b");

        // Neither should be fully Unconstrained due to the veto conditions
        assert!(
            !matches!(a_domain, Domain::Unconstrained),
            "solution {}: a should NOT be Unconstrained - veto constrains a >= 0",
            i
        );
        assert!(
            !matches!(b_domain, Domain::Unconstrained),
            "solution {}: b should NOT be Unconstrained - veto constrains b >= 0",
            i
        );
    }
}

#[test]
fn form_validation_should_produce_correct_constraints() {
    // Real-world scenario: validate form inputs
    // Rule: can_place_order = true
    //   unless item_count < 1 then veto
    //   unless item_count > 100 then veto
    //   unless (shipping_method is "standard" or shipping_method is "express") then veto
    // Expected: item_count in [1, 100], shipping_method in {"standard", "express"}
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

    let response = engine
        .invert_strict(
            "order",
            "can_place_order",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domain");

    assert_eq!(response.len(), 1, "should have one solution");

    let solution = &response.solutions[0];
    let item_count_path = FactPath::local("item_count".to_string());
    let shipping_path = FactPath::local("shipping_method".to_string());

    // Check item_count: should be [1, 100]
    let item_domain = solution
        .get(&item_count_path)
        .expect("solution should contain item_count");

    match item_domain {
        Domain::Range { min, max } => {
            match min {
                Bound::Inclusive(v) => {
                    assert_eq!(*v, LiteralValue::number(1), "item_count min should be 1")
                }
                other => panic!("item_count min should be Inclusive(1), got {:?}", other),
            }
            match max {
                Bound::Inclusive(v) => assert_eq!(
                    *v,
                    LiteralValue::number(100),
                    "item_count max should be 100"
                ),
                other => panic!("item_count max should be Inclusive(100), got {:?}", other),
            }
        }
        Domain::Unconstrained => {
            panic!("item_count should NOT be Unconstrained - vetos constrain it to [1, 100]");
        }
        other => panic!("expected item_count Range [1, 100], got {:?}", other),
    }

    // Check shipping_method: should be enumeration of {"standard", "express"}
    let shipping_domain = solution
        .get(&shipping_path)
        .expect("solution should contain shipping_method");

    match shipping_domain {
        Domain::Enumeration(values) => {
            assert_eq!(
                values.len(),
                2,
                "shipping_method should have 2 valid values"
            );
            let has_standard = values
                .iter()
                .any(|v| matches!(v, LiteralValue::Text(s) if s == "standard"));
            let has_express = values
                .iter()
                .any(|v| matches!(v, LiteralValue::Text(s) if s == "express"));
            assert!(has_standard, "shipping_method should include 'standard'");
            assert!(has_express, "shipping_method should include 'express'");
        }
        Domain::Unconstrained => {
            panic!("shipping_method should NOT be Unconstrained - veto constrains it to standard/express");
        }
        other => panic!("expected shipping_method Enumeration, got {:?}", other),
    }
}
