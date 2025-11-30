#![cfg(feature = "inversion")]

use lemma::{BooleanValue, Bound, Domain, Engine, FactPath, LiteralValue, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn inversion_single_unknown_should_solve_algebraically() {
    // Given: total = price * quantity, quantity = 5, target total = 50
    // Expected: price = 10 (algebraically solved: 50 / 5 = 10)
    let code = r#"
        doc pricing
        fact price = [number]
        fact quantity = [number]
        rule total = price * quantity
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert("quantity".to_string(), LiteralValue::number(5));

    let response = engine
        .invert_strict(
            "pricing",
            "total",
            Target::value(LiteralValue::number(50)),
            given,
        )
        .expect("invert should succeed");

    assert_eq!(response.len(), 1, "should have exactly one solution");

    let solution = &response.solutions[0];
    let price_path = FactPath::local("price".to_string());

    assert!(
        solution.contains_key(&price_path),
        "solution should contain price"
    );

    let price_domain = solution.get(&price_path).unwrap();
    // Price should be exactly 10, not Unconstrained
    assert_eq!(
        *price_domain,
        Domain::Enumeration(vec![LiteralValue::number(10)]),
        "price should be exactly 10 (50 / 5 = 10), not {:?}",
        price_domain
    );
}

#[test]
fn inversion_fully_constrained_should_have_no_free_vars() {
    let code = r#"
        doc pricing
        fact price = [number]
        fact quantity = [number]
        rule total = price * quantity
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert("price".to_string(), LiteralValue::number(10));
    given.insert("quantity".to_string(), LiteralValue::number(5));

    let response = engine
        .invert_strict(
            "pricing",
            "total",
            Target::value(LiteralValue::number(50)),
            given,
        )
        .expect("invert should succeed");

    assert!(
        response.is_fully_constrained,
        "should be fully constrained when all facts are provided"
    );
    assert!(
        response.free_variables.is_empty(),
        "should have no free variables"
    );
    // Solutions should be empty or contain no domains since everything is given
}

#[test]
fn inversion_veto_boundary_should_produce_range() {
    // Rule: can_ship = true unless weight > 100kg then veto
    // Target: any valid value (not veto)
    // Expected: weight <= 100 (or weight in range (-inf, 100])
    let code = r#"
        doc shipping
        fact weight = [mass]
        
        rule can_ship = true
          unless weight > 100 kilograms then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("shipping", "can_ship", Target::any_value(), HashMap::new())
        .expect("invert should succeed");

    assert!(
        !response.free_variables.is_empty(),
        "should have weight as free variable"
    );
    assert_eq!(response.len(), 1, "should have one solution");

    let solution = &response.solutions[0];
    let weight_path = FactPath::local("weight".to_string());

    assert!(
        solution.contains_key(&weight_path),
        "solution should contain weight"
    );

    let weight_domain = solution.get(&weight_path).unwrap();
    // Weight should be constrained to <= 100, not Unconstrained
    // Could be either Range(-inf, 100] or Complement(Range(100, inf))
    match weight_domain {
        Domain::Range { min, max } => {
            assert!(matches!(min, Bound::Unbounded), "min should be unbounded");
            match max {
                Bound::Inclusive(v) => {
                    // Accept either number or unit value for 100
                    let is_100 = match v {
                        LiteralValue::Number(n) => *n == rust_decimal::Decimal::from(100),
                        LiteralValue::Unit(u) => u.value() == rust_decimal::Decimal::from(100),
                        _ => false,
                    };
                    assert!(is_100, "max should be 100, got {:?}", v);
                }
                other => panic!("max should be Inclusive(100), got {:?}", other),
            }
        }
        Domain::Complement(inner) => {
            // Complement of (100, inf) is also valid representation of <= 100
            match inner.as_ref() {
                Domain::Range { min, max } => {
                    match min {
                        Bound::Exclusive(v) => {
                            let is_100 = match v {
                                LiteralValue::Number(n) => *n == rust_decimal::Decimal::from(100),
                                LiteralValue::Unit(u) => {
                                    u.value() == rust_decimal::Decimal::from(100)
                                }
                                _ => false,
                            };
                            assert!(is_100, "complement range min should be 100, got {:?}", v);
                        }
                        other => panic!(
                            "complement range min should be Exclusive(100), got {:?}",
                            other
                        ),
                    }
                    assert!(
                        matches!(max, Bound::Unbounded),
                        "complement range max should be unbounded"
                    );
                    // This is valid but not ideal - domain normalization should simplify this
                    println!(
                        "WARNING: Domain is Complement(Range) - should be normalized to Range"
                    );
                }
                other => panic!("expected Complement(Range), got Complement({:?})", other),
            }
        }
        Domain::Unconstrained => {
            panic!("weight should NOT be Unconstrained - it should be <= 100 kg");
        }
        other => panic!("expected Range or Complement(Range), got {:?}", other),
    }
}

#[test]
fn inversion_multiple_boolean_facts_should_produce_multiple_solutions() {
    // Rule with multiple unless clauses based on boolean facts
    // discount = 0%
    //   unless is_member then 10%
    //   unless is_premium then 20%
    //   unless has_coupon then 5%
    //
    // Target: 10% discount
    // Expected solutions:
    //   1. is_member = true, is_premium = false, has_coupon = false (10% from membership)
    //   2. is_member = false, is_premium = false, has_coupon = true, AND some other condition?
    //      Actually with "last wins" semantics, to get exactly 10%:
    //      - is_member must be true (to trigger the 10% branch)
    //      - is_premium must be false (otherwise we'd get 20%)
    //      - has_coupon must be false (otherwise we'd get 5%)
    //
    // So there should be exactly ONE solution for target = 10%
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

    // Invert for 10% discount
    let response = engine
        .invert_strict(
            "shop",
            "discount",
            Target::value(LiteralValue::Percentage(Decimal::from(10))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    println!("Solutions for 10% discount:");
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }
    println!("Free variables: {:?}", response.free_variables);

    // For 10% discount with "last wins" semantics:
    // - is_member must be true (triggers 10% branch)
    // - is_premium must be false (would override to 20%)
    // - has_coupon must be false (would override to 5%)
    assert_eq!(
        response.len(),
        1,
        "should have exactly 1 solution for 10% discount"
    );

    let solution = &response.solutions[0];
    let is_member_path = FactPath::local("is_member".to_string());
    let is_premium_path = FactPath::local("is_premium".to_string());
    let has_coupon_path = FactPath::local("has_coupon".to_string());

    // is_member should be constrained to true
    let is_member_domain = solution
        .get(&is_member_path)
        .expect("solution should contain is_member");
    assert_eq!(
        *is_member_domain,
        Domain::Enumeration(vec![LiteralValue::Boolean(BooleanValue::True)]),
        "is_member should be exactly true, got {:?}",
        is_member_domain
    );

    // is_premium should be constrained to false
    let is_premium_domain = solution
        .get(&is_premium_path)
        .expect("solution should contain is_premium");
    assert_eq!(
        *is_premium_domain,
        Domain::Enumeration(vec![LiteralValue::Boolean(BooleanValue::False)]),
        "is_premium should be exactly false, got {:?}",
        is_premium_domain
    );

    // has_coupon should be constrained to false
    let has_coupon_domain = solution
        .get(&has_coupon_path)
        .expect("solution should contain has_coupon");
    assert_eq!(
        *has_coupon_domain,
        Domain::Enumeration(vec![LiteralValue::Boolean(BooleanValue::False)]),
        "has_coupon should be exactly false, got {:?}",
        has_coupon_domain
    );
}

#[test]
fn inversion_any_value_with_multiple_branches_should_produce_multiple_solutions() {
    // Same rule, but target is "any value" (not veto)
    // This should produce 4 solutions (one for each possible discount outcome):
    //   1. 0%  - when none of the conditions match
    //   2. 10% - when is_member=true and later conditions don't override
    //   3. 20% - when is_premium=true and has_coupon doesn't override
    //   4. 5%  - when has_coupon=true (last wins)
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
        .invert_strict("shop", "discount", Target::any_value(), HashMap::new())
        .expect("invert should succeed");

    println!("\nSolutions for ANY discount value:");
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }
    println!("Shape branches: {}", response.shape.branches.len());

    // Should have 4 solutions (one per outcome: 0%, 10%, 20%, 5%)
    assert_eq!(
        response.len(),
        4,
        "should have 4 solutions (one per discount tier), got {}",
        response.len()
    );

    // Each solution should have constraints on the boolean facts, not all Unconstrained
    let is_member_path = FactPath::local("is_member".to_string());
    let is_premium_path = FactPath::local("is_premium".to_string());
    let has_coupon_path = FactPath::local("has_coupon".to_string());

    let mut found_constrained_solution = false;
    for (i, solution) in response.solutions.iter().enumerate() {
        let is_member = solution.get(&is_member_path);
        let is_premium = solution.get(&is_premium_path);
        let has_coupon = solution.get(&has_coupon_path);

        // At least ONE solution should have non-Unconstrained domains
        // (the last branch "has_coupon then 5%" should constrain has_coupon=true)
        let has_constraint = [is_member, is_premium, has_coupon]
            .iter()
            .any(|d| d.is_some() && !matches!(d.unwrap(), Domain::Unconstrained));

        if has_constraint {
            found_constrained_solution = true;
            println!("  Solution {} has actual constraints!", i + 1);
        }
    }

    assert!(
        found_constrained_solution,
        "At least one solution should have constrained boolean facts, not all Unconstrained. \
         For example, 5% discount requires has_coupon=true"
    );
}

#[test]
fn inversion_enum_fact_should_show_valid_values() {
    // Rule with text comparisons that effectively create an enumeration
    // status can be "pending", "approved", "rejected"
    // outcome depends on status
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

    // Invert for can_proceed = true
    // Expected: status = "approved" only
    let response = engine
        .invert_strict(
            "workflow",
            "can_proceed",
            Target::value(LiteralValue::Boolean(BooleanValue::True)),
            HashMap::new(),
        )
        .expect("invert should succeed");

    println!("\nSolutions for can_proceed = true:");
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    assert_eq!(response.len(), 1, "should have exactly 1 solution");

    let solution = &response.solutions[0];
    let status_path = FactPath::local("status".to_string());

    let status_domain = solution
        .get(&status_path)
        .expect("solution should contain status");

    // status should be exactly "approved"
    assert_eq!(
        *status_domain,
        Domain::Enumeration(vec![LiteralValue::Text("approved".to_string())]),
        "status should be exactly 'approved', got {:?}",
        status_domain
    );
}

#[test]
fn inversion_veto_target_should_show_veto_conditions() {
    // Same rule, but invert for "any veto"
    // Expected: 2 solutions (pending or rejected)
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
            Target::any_veto(),
            HashMap::new(),
        )
        .expect("invert should succeed");

    println!("\nSolutions for can_proceed = veto:");
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    // Should have 2 solutions: status="pending" or status="rejected"
    assert_eq!(
        response.len(),
        2,
        "should have 2 veto solutions (pending, rejected), got {}",
        response.len()
    );

    let status_path = FactPath::local("status".to_string());

    // Collect all status values from solutions
    let mut status_values: Vec<String> = Vec::new();
    for solution in &response.solutions {
        if let Some(Domain::Enumeration(vals)) = solution.get(&status_path) {
            for v in vals {
                if let LiteralValue::Text(s) = v {
                    status_values.push(s.clone());
                }
            }
        }
    }

    assert!(
        status_values.contains(&"pending".to_string()),
        "should have solution for status='pending'"
    );
    assert!(
        status_values.contains(&"rejected".to_string()),
        "should have solution for status='rejected'"
    );
}

// =============================================================================
// COMPLEX REAL-WORLD SCENARIOS
// =============================================================================

#[test]
fn complex_pricing_with_member_coupon_combo() {
    // Real-world pricing:
    // - Base discount depends on membership AND coupon combinations
    // - Members get 10%, coupons give 5%, but members WITH coupons get 20% (not just 15%)
    // - Premium members get 25% regardless of coupon
    //
    // Target: Find all ways to get exactly 20% discount
    // Expected solutions:
    //   1. is_member=true AND has_coupon=true AND is_premium=false
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
            Target::value(LiteralValue::Percentage(Decimal::from(20))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    println!("\n=== Complex: 20% discount solutions ===");
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    // Should have exactly 1 solution: member + coupon + not premium
    assert_eq!(
        response.len(),
        1,
        "should have exactly 1 way to get 20% discount (member+coupon combo)"
    );

    let solution = &response.solutions[0];
    let is_member = solution.get(&FactPath::local("is_member".to_string()));
    let has_coupon = solution.get(&FactPath::local("has_coupon".to_string()));
    let is_premium = solution.get(&FactPath::local("is_premium".to_string()));

    // Verify constraints
    assert_eq!(
        is_member,
        Some(&Domain::Enumeration(vec![LiteralValue::Boolean(
            BooleanValue::True
        )])),
        "is_member should be true for 20% combo discount"
    );
    assert_eq!(
        has_coupon,
        Some(&Domain::Enumeration(vec![LiteralValue::Boolean(
            BooleanValue::True
        )])),
        "has_coupon should be true for 20% combo discount"
    );
    assert_eq!(
        is_premium,
        Some(&Domain::Enumeration(vec![LiteralValue::Boolean(
            BooleanValue::False
        )])),
        "is_premium should be false (otherwise we'd get 25%)"
    );
}

#[test]
fn complex_final_price_with_trade_in_and_discount() {
    // Complex price calculation:
    // - final_price = (base_price - trade_in_value) * (1 - discount_rate)
    // - discount_rate depends on membership and order size
    // - trade_in_value depends on item condition
    //
    // Target: Find inputs that give final_price = 80
    let code = r#"
        doc order
        fact base_price = [number]
        fact has_trade_in = [boolean]
        fact trade_in_condition = [text]
        fact is_member = [boolean]
        fact order_quantity = [number]
        
        rule trade_in_value = 0
          unless has_trade_in and trade_in_condition == "excellent" then 50
          unless has_trade_in and trade_in_condition == "good" then 30
          unless has_trade_in and trade_in_condition == "fair" then 10
        
        rule discount_rate = 0%
          unless is_member then 10%
          unless order_quantity >= 5 then 15%
          unless is_member and order_quantity >= 10 then 25%
        
        rule subtotal = base_price - trade_in_value?
        
        rule final_price = subtotal? - (subtotal? * discount_rate?)
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // First, let's see what solutions exist for any valid final_price
    let response = engine
        .invert_strict("order", "final_price", Target::any_value(), HashMap::new())
        .expect("invert should succeed");

    println!("\n=== Complex: All valid final_price solutions ===");
    println!("Number of solutions: {}", response.len());
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    // Should have multiple solutions based on different discount/trade-in combos
    assert!(
        response.len() >= 4,
        "should have multiple solutions for different discount/trade-in combinations, got {}",
        response.len()
    );

    // Each solution should have constrained facts (not all Unconstrained)
    for (i, solution) in response.solutions.iter().enumerate() {
        let all_unconstrained = solution
            .values()
            .all(|d| matches!(d, Domain::Unconstrained));
        assert!(
            !all_unconstrained,
            "Solution {} should have at least one constrained fact",
            i + 1
        );
    }
}

#[test]
fn complex_eligibility_with_multiple_conditions() {
    // Loan eligibility with multiple interdependent conditions:
    // - Must be 18-65 years old
    // - Income must be >= 30000 OR have a co-signer
    // - Credit score affects the rate but blocks if too low
    // - Existing customer gets better terms
    //
    // Target: Find all ways to be eligible (not veto)
    let code = r#"
        doc loan
        fact age = [number]
        fact income = [number]
        fact has_cosigner = [boolean]
        fact credit_score = [number]
        fact is_existing_customer = [boolean]
        
        rule eligible = true
          unless age < 18 then veto "too young"
          unless age > 65 then veto "exceeds age limit"
          unless income < 30000 and not has_cosigner then veto "insufficient income without cosigner"
          unless credit_score < 500 then veto "credit score too low"
          unless credit_score < 600 and not is_existing_customer then veto "new customers need credit >= 600"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("loan", "eligible", Target::any_value(), HashMap::new())
        .expect("invert should succeed");

    println!("\n=== Complex: Loan eligibility solutions ===");
    println!("Number of solutions: {}", response.len());
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    // Should have solutions with proper constraints
    assert!(!response.is_empty(), "should have eligibility solutions");

    let age_path = FactPath::local("age".to_string());
    // Check that age is constrained to [18, 65]
    for solution in &response.solutions {
        if let Some(age_domain) = solution.get(&age_path) {
            assert!(
                !matches!(age_domain, Domain::Unconstrained),
                "age should be constrained to [18, 65], not Unconstrained"
            );
        }
    }
}

#[test]
fn complex_shipping_with_weight_size_destination() {
    // Shipping rules with multiple dimensions:
    // - Weight and size both affect shippability
    // - Some destinations have stricter limits
    // - Express shipping has even stricter limits
    // - Oversized items need special handling
    //
    // Target: Find all inputs that allow shipping (no veto)
    let code = r#"
        doc shipping
        fact weight = [number]
        fact length = [number]
        fact width = [number]
        fact height = [number]
        fact destination = [text]
        fact is_express = [boolean]
        
        rule volume = length * width * height
        
        rule dimensional_weight = volume? / 5000
        
        rule billable_weight = weight
          unless dimensional_weight? > weight then dimensional_weight?
        
        rule can_ship = true
          unless weight > 70 then veto "exceeds weight limit"
          unless billable_weight? > 50 and is_express then veto "too heavy for express"
          unless volume? > 100000 then veto "exceeds size limit"
          unless destination == "international" and weight > 30 then veto "international weight limit"
          unless destination == "international" and volume? > 50000 then veto "international size limit"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("shipping", "can_ship", Target::any_value(), HashMap::new())
        .expect("invert should succeed");

    println!("\n=== Complex: Shipping feasibility solutions ===");
    println!("Number of solutions: {}", response.len());
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    // Should have solutions with constraints on weight, volume, etc.
    assert!(!response.is_empty(), "should have shipping solutions");

    let weight_path = FactPath::local("weight".to_string());

    // Weight should be constrained (at minimum <= 70)
    for (i, solution) in response.solutions.iter().enumerate() {
        if let Some(weight_domain) = solution.get(&weight_path) {
            assert!(
                !matches!(weight_domain, Domain::Unconstrained),
                "Solution {}: weight should be constrained (max 70), not Unconstrained",
                i + 1
            );
        }
    }
}

#[test]
fn complex_insurance_quote_with_risk_factors() {
    // Insurance pricing with multiple risk factors that interact:
    // - Base rate modified by age brackets
    // - Smoker status affects rate
    // - Pre-existing conditions may disqualify or increase rate
    // - Family history considered for some conditions
    //
    // Target: Find inputs that result in "standard" tier (not veto, not high-risk)
    let code = r#"
        doc insurance
        fact age = [number]
        fact is_smoker = [boolean]
        fact has_preexisting = [boolean]
        fact preexisting_type = [text]
        fact has_family_history = [boolean]
        
        rule risk_tier = "standard"
          unless age < 18 then veto "must be 18+"
          unless age > 80 then veto "exceeds coverage age"
          unless is_smoker and age > 50 then "high-risk"
          unless has_preexisting and preexisting_type == "severe" then veto "uninsurable condition"
          unless has_preexisting and preexisting_type == "moderate" then "high-risk"
          unless has_family_history and age > 40 then "elevated"
          unless is_smoker then "elevated"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Find all ways to get "standard" tier
    let response = engine
        .invert_strict(
            "insurance",
            "risk_tier",
            Target::value(LiteralValue::Text("standard".to_string())),
            HashMap::new(),
        )
        .expect("invert should succeed");

    println!("\n=== Complex: 'standard' insurance tier solutions ===");
    println!("Number of solutions: {}", response.len());
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    // Should find solutions where:
    // - age in [18, 80]
    // - is_smoker = false (otherwise elevated or high-risk)
    // - has_preexisting = false OR preexisting_type != "moderate"/"severe"
    // - has_family_history = false OR age <= 40
    assert!(!response.is_empty(), "should have standard tier solutions");

    let is_smoker_path = FactPath::local("is_smoker".to_string());

    // Verify age constraint is extracted (should be [18, 80] combining both veto conditions)
    // Note: current implementation may only capture partial constraint
    let age_path = FactPath::local("age".to_string());
    for (i, solution) in response.solutions.iter().enumerate() {
        if let Some(age_domain) = solution.get(&age_path) {
            println!("  Age domain in solution {}: {:?}", i + 1, age_domain);
            assert!(
                !matches!(age_domain, Domain::Unconstrained),
                "Solution {}: age should be constrained (18-80), not Unconstrained",
                i + 1
            );
        }
    }

    // is_smoker should be constrained to false for standard tier
    // (smokers get "elevated" at minimum due to the last unless clause)
    for (i, solution) in response.solutions.iter().enumerate() {
        if let Some(smoker_domain) = solution.get(&is_smoker_path) {
            println!(
                "  is_smoker domain in solution {}: {:?}",
                i + 1,
                smoker_domain
            );
            // Should be false (not Unconstrained) because smokers get "elevated" at minimum
            assert!(
                !matches!(smoker_domain, Domain::Unconstrained),
                "Solution {}: is_smoker should be false for standard tier, not Unconstrained. \
                 The 'unless is_smoker then elevated' clause should constrain this.",
                i + 1
            );
        }
    }
}

#[test]
fn complex_rule_references_should_expand_properly() {
    // Test that rule references (?) are properly expanded during inversion
    // This is critical for multi-step calculations
    //
    // rate depends on tier
    // tier depends on points
    // final_amount = base * rate
    //
    // Target: Find points that give rate = 15%
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

    // Invert rate for 15% - should tell us points >= 500 AND points < 1000 (gold tier)
    let response = engine
        .invert_strict(
            "rewards",
            "rate",
            Target::value(LiteralValue::Percentage(Decimal::from(15))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    println!("\n=== Rule References: 15% rate solutions ===");
    println!("Number of solutions: {}", response.len());
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    assert_eq!(
        response.len(),
        1,
        "should have exactly 1 solution for 15% rate (gold tier)"
    );

    let solution = &response.solutions[0];
    let points_path = FactPath::local("points".to_string());

    let points_domain = solution
        .get(&points_path)
        .expect("solution should contain points constraint");

    println!("Points domain: {:?}", points_domain);

    // Points should be in range [500, 1000) for gold tier
    match points_domain {
        Domain::Range { min, max } => {
            match min {
                Bound::Inclusive(v) => {
                    assert_eq!(*v, LiteralValue::number(500), "points min should be 500");
                }
                other => panic!("points min should be Inclusive(500), got {:?}", other),
            }
            match max {
                Bound::Exclusive(v) => {
                    assert_eq!(
                        *v,
                        LiteralValue::number(1000),
                        "points max should be 1000 exclusive"
                    );
                }
                other => panic!("points max should be Exclusive(1000), got {:?}", other),
            }
        }
        Domain::Unconstrained => {
            panic!(
                "points should be constrained to [500, 1000) for gold tier, not Unconstrained. \
                 The rule reference tier? should be expanded to derive points constraints."
            );
        }
        other => panic!("expected Range [500, 1000), got {:?}", other),
    }
}

#[test]
fn complex_chained_rules_with_arithmetic() {
    // Test arithmetic through chained rules
    // gross = hours * rate
    // deductions = gross * tax_rate
    // net = gross - deductions
    //
    // Given: hours=40, rate=25, tax_rate=20%
    // gross = 40 * 25 = 1000
    // deductions = 1000 * 0.2 = 200
    // net = 1000 - 200 = 800
    //
    // Invert: what hours give net = 800 when rate=25, tax_rate=20%?
    // Answer: hours = 40
    let code = r#"
        doc payroll
        fact hours = [number]
        fact rate = [number]
        fact tax_rate = [number]
        
        rule gross = hours * rate
        rule deductions = gross? * tax_rate
        rule net = gross? - deductions?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert("rate".to_string(), LiteralValue::number(25));
    // tax_rate is a number (0.2 = 20%), not a percentage type
    given.insert(
        "tax_rate".to_string(),
        LiteralValue::Number(Decimal::new(2, 1)),
    ); // 0.2

    let response = engine
        .invert_strict(
            "payroll",
            "net",
            Target::value(LiteralValue::number(800)),
            given,
        )
        .expect("invert should succeed");

    println!("\n=== Chained Arithmetic: net=800 solutions ===");
    println!("Number of solutions: {}", response.len());
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    assert_eq!(response.len(), 1, "should have exactly 1 solution");

    let solution = &response.solutions[0];
    let hours_path = FactPath::local("hours".to_string());

    let hours_domain = solution
        .get(&hours_path)
        .expect("solution should contain hours");

    // hours should be exactly 40
    // net = gross - deductions = hours*rate - hours*rate*tax_rate = hours*rate*(1-tax_rate)
    // 800 = hours * 25 * 0.8 = hours * 20
    // hours = 40
    assert_eq!(
        *hours_domain,
        Domain::Enumeration(vec![LiteralValue::number(40)]),
        "hours should be exactly 40 (800 / (25 * 0.8)), got {:?}",
        hours_domain
    );
}

#[test]
fn complex_event_booking_with_capacity_and_timing() {
    // Event booking with multiple constraints:
    // - Venue capacity limits
    // - Time slot availability (peak vs off-peak)
    // - Member vs non-member booking windows
    // - Special events have different rules
    //
    // Target: Find all valid booking configurations
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
          unless is_peak_time and is_special_event and not is_member then veto "members only for peak special events"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict("booking", "can_book", Target::any_value(), HashMap::new())
        .expect("invert should succeed");

    println!("\n=== Complex: Event booking solutions ===");
    println!("Number of solutions: {}", response.len());
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, solution);
    }

    // Should have multiple solutions based on venue/timing/membership combos
    assert!(
        !response.is_empty(),
        "should have valid booking configurations"
    );

    let attendee_path = FactPath::local("attendee_count".to_string());
    let advance_path = FactPath::local("days_in_advance".to_string());

    // attendee_count should be constrained (>= 1, <= capacity)
    // days_in_advance should be constrained (>= min_advance_days)
    for (i, solution) in response.solutions.iter().enumerate() {
        let attendee_domain = solution.get(&attendee_path);
        let advance_domain = solution.get(&advance_path);

        // At least one of these should be constrained
        let has_constraint = [attendee_domain, advance_domain]
            .iter()
            .any(|d| d.is_some() && !matches!(d.unwrap(), Domain::Unconstrained));

        assert!(
            has_constraint,
            "Solution {}: should have constraints on attendee_count or days_in_advance",
            i + 1
        );
    }
}

#[test]
fn numeric_contradiction_range_and_enumeration_should_be_filtered() {
    // Test: (x <= 3) AND (x == 7) should be filtered out (unsatisfiable)
    let code = r#"
        doc test
        fact x = [number]
        rule result = x
          unless x <= 3 then veto "too small"
          unless x == 7 then veto "must be 7"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: result == 7 should fail because x <= 3 AND x == 7 is contradictory
    let result = engine.invert_strict(
        "test",
        "result",
        Target::value(LiteralValue::number(7)),
        HashMap::new(),
    );

    assert!(
        result.is_err(),
        "Should fail: (x <= 3) AND (x == 7) is contradictory"
    );
}

#[test]
fn numeric_contradiction_two_equalities_should_be_filtered() {
    // Test: (x == 1) AND (x == 2) should be filtered out (unsatisfiable)
    let code = r#"
        doc test
        fact x = [number]
        rule result = x
          unless x == 1 then veto "must be 1"
          unless x == 2 then veto "must be 2"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: result == 1 should fail because x == 1 AND x == 2 is contradictory
    let result = engine.invert_strict(
        "test",
        "result",
        Target::value(LiteralValue::number(1)),
        HashMap::new(),
    );

    assert!(
        result.is_err(),
        "Should fail: (x == 1) AND (x == 2) is contradictory"
    );
}

#[test]
fn numeric_contradiction_algebraic_and_range_should_be_filtered() {
    // Test: (price * 5 = 50) AND (price < 5) should be filtered out
    // price * 5 = 50 → price = 10 (algebraically solved), but price < 5 is contradictory
    // We need a rule where the condition actually requires price < 5
    let code = r#"
        doc test
        fact price = [number]
        rule total = price * 5
          unless price >= 5 then veto "price must be < 5"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: total == 50 should fail because:
    // - price * 5 = 50 → price = 10 (algebraically solved)
    // - Default branch condition: NOT(price >= 5) = price < 5
    // - price = 10 contradicts price < 5
    let result = engine.invert_strict(
        "test",
        "total",
        Target::value(LiteralValue::number(50)),
        HashMap::new(),
    );

    assert!(
        result.is_err(),
        "Should fail: (price * 5 = 50) AND (price < 5) is contradictory (price = 10 contradicts price < 5)"
    );
}

#[test]
fn numeric_satisfiable_condition_should_pass() {
    // Test: (x >= 0) AND (x <= 10) AND (x == 5) should pass (satisfiable)
    // Rule: result = x with constraints x >= 0 AND x <= 10
    let code = r#"
        doc test
        fact x = [number]
        rule result = x
          unless x < 0 then veto "negative"
          unless x > 10 then veto "too large"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: result == 5 should succeed because:
    // - Default branch condition: NOT(x < 0) AND NOT(x > 10) = (x >= 0) AND (x <= 10)
    // - Target: x == 5
    // - Combined: (x >= 0) AND (x <= 10) AND (x == 5) is satisfiable
    let result = engine.invert_strict(
        "test",
        "result",
        Target::value(LiteralValue::number(5)),
        HashMap::new(),
    );

    assert!(
        result.is_ok(),
        "Should succeed: (x >= 0) AND (x <= 10) AND (x == 5) is satisfiable"
    );

    let response = result.unwrap();
    assert_eq!(response.len(), 1, "should have one solution");
    let solution = &response.solutions[0];
    let x_path = FactPath::local("x".to_string());
    let x_domain = solution.get(&x_path).expect("solution should contain x");
    assert_eq!(
        *x_domain,
        Domain::Enumeration(vec![LiteralValue::number(5)]),
        "x should be exactly 5"
    );
}

#[test]
fn numeric_or_condition_should_pass() {
    // Test: OR conditions in domain extraction should work correctly
    // Rule: result = x with condition (x > 3 OR x < 0) - no veto
    // This tests that OR conditions don't create false contradictions
    let code = r#"
        doc test
        fact x = [number]
        rule result = x
          unless (x <= 3 and x >= 0) then veto "must be > 3 or < 0"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: result == 5 should succeed because:
    // - Default branch condition: NOT(x <= 3 and x >= 0) = (x > 3 OR x < 0)
    // - Target: x == 5
    // - Combined: (x > 3 OR x < 0) AND (x == 5) = (x > 3 AND x == 5) which is satisfiable
    let result = engine.invert_strict(
        "test",
        "result",
        Target::value(LiteralValue::number(5)),
        HashMap::new(),
    );

    assert!(
        result.is_ok(),
        "Should succeed: (x > 3 OR x < 0) AND (x == 5) is satisfiable (x == 5 satisfies x > 3)"
    );

    let response = result.unwrap();
    println!("\n=== Inversion Response for result == 5 ===");
    println!("Number of solutions: {}", response.len());
    for (i, solution) in response.solutions.iter().enumerate() {
        println!("\nSolution {}:", i + 1);
        for (fact_path, domain) in solution.iter() {
            println!("  {}: {:?}", fact_path, domain);
            match domain {
                Domain::Range { min, max } => {
                    println!("    -> Range: min={:?}, max={:?}", min, max);
                }
                Domain::Enumeration(vals) => {
                    println!("    -> Enumeration: {:?}", vals);
                }
                Domain::Union(parts) => {
                    println!("    -> Union with {} parts", parts.len());
                }
                Domain::Complement(inner) => {
                    println!("    -> Complement of: {:?}", inner);
                }
                Domain::Unconstrained => {
                    println!("    -> Unconstrained");
                }
            }
        }
    }
    println!("==========================================\n");
}
