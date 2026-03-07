use lemma::{Engine, FactPath, LiteralValue, Target, TargetOp};
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use std::collections::HashMap;

/// Test TargetOp::Gt (Greater Than)
#[test]
fn target_operator_greater_than() {
    let code = r#"
        spec pricing
        fact base_price: [number]
        fact markup_rate: 1.5

        rule final_price: base_price * markup_rate
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Question: "What base prices result in final price > $100?"
    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "pricing",
            &now,
            "final_price",
            Target::with_op(
                TargetOp::Gt,
                lemma::OperationResult::Value(Box::new(LiteralValue::number(100.into()))),
            ),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should have at least one solution
    assert!(!solutions.is_empty(), "should have solutions");

    // Should track base_price in domain
    let base_price_path = FactPath::local("base_price".to_string());
    assert!(
        solutions
            .domains
            .iter()
            .any(|d| d.contains_key(&base_price_path)),
        "base_price should be in domains"
    );
}

/// Test TargetOp::Lte (Less Than or Equal)
#[test]
fn target_operator_less_than_or_equal() {
    let code = r#"
        spec budget
        fact monthly_cost: [number]
        fact months: 12

        rule annual_cost: monthly_cost * months
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Question: "What monthly costs keep annual cost <= $50,000?"
    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "budget",
            &now,
            "annual_cost",
            Target::with_op(
                TargetOp::Lte,
                lemma::OperationResult::Value(Box::new(LiteralValue::number(50000.into()))),
            ),
            HashMap::new(),
        )
        .expect("should invert successfully");

    let monthly_cost_path = FactPath::local("monthly_cost".to_string());
    assert!(
        solutions
            .domains
            .iter()
            .any(|d| d.contains_key(&monthly_cost_path)),
        "monthly_cost should be a free variable"
    );
}

/// Test TargetOp::Gte (Greater Than or Equal)
#[test]
fn target_operator_greater_than_or_equal() {
    let code = r#"
        spec compensation
        fact base_salary: [number]
        fact bonus_rate: 0.20

        rule total_comp: base_salary * (1 + bonus_rate)
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Question: "What base salaries give total comp >= $120,000?"
    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "compensation",
            &now,
            "total_comp",
            Target::with_op(
                TargetOp::Gte,
                lemma::OperationResult::Value(Box::new(LiteralValue::number(120000.into()))),
            ),
            HashMap::new(),
        )
        .expect("should invert successfully");

    let base_salary_path = FactPath::local("base_salary".to_string());
    assert!(
        solutions
            .domains
            .iter()
            .any(|d| d.contains_key(&base_salary_path)),
        "base_salary should be a free variable"
    );
}

/// Test Boolean NOT operator in conditions
#[test]
fn boolean_not_operator() {
    let code = r#"
        spec eligibility
        fact is_suspended: [boolean]
        fact has_membership: [boolean]

        rule can_access: true
          unless not has_membership then veto "Must be a member"
          unless is_suspended then veto "Account suspended"
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Question: "What conditions trigger veto?"
    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "eligibility",
            &now,
            "can_access",
            Target::any_veto(),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should have solutions
    assert!(!solutions.is_empty(), "should have solutions");

    // Should track boolean variables in domains
    assert!(
        solutions.domains.iter().any(|d| {
            d.keys()
                .any(|k| k.fact.contains("is_suspended") || k.fact.contains("has_membership"))
        }),
        "should track boolean condition variables"
    );
}

/// Test cross-spec inversion - Simple case
#[test]
fn cross_spec_simple() {
    let base_spec = r#"
        spec base
        fact discount_rate: 0.15
    "#;

    let derived_spec = r#"
        spec derived
        fact base: spec base
        fact order_total: [number]

        rule discount: order_total * base.discount_rate
        rule final_total: order_total - discount
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, base_spec, "base").unwrap();
    add_lemma_code_blocking(&mut engine, derived_spec, "derived").unwrap();

    // Question: "What order_total gives final_total of $85?"
    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "derived",
            &now,
            "final_total",
            Target::value(LiteralValue::number(85.into())),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should solve algebraically: order_total = 85 / 0.85 = 100
    let order_total_path = FactPath::local("order_total".to_string());
    assert!(
        solutions.domains.iter().all(|d| d.is_empty())
            || solutions
                .domains
                .iter()
                .any(|d| d.contains_key(&order_total_path)),
        "order_total should be referenced or fully solved"
    );
}

/// Test cross-spec inversion - Rule references across specs
#[test]
fn cross_spec_rule_references() {
    let config_spec = r#"
        spec config
        fact min_threshold: 1000

        rule eligibility_threshold: min_threshold * 2
    "#;

    let order_spec = r#"
        spec order
        fact settings: spec config
        fact customer_lifetime_value: [number]

        rule is_vip: customer_lifetime_value >= settings.eligibility_threshold
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, config_spec, "config").unwrap();
    add_lemma_code_blocking(&mut engine, order_spec, "order").unwrap();

    let mut given = HashMap::new();
    given.insert("settings.min_threshold".to_string(), "1000".to_string());

    // Question: "What customer_lifetime_value makes is_vip true?" (>= 2000)
    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "order",
            &now,
            "is_vip",
            Target::value(LiteralValue::from_bool(true)),
            given,
        )
        .expect("should invert successfully");

    // Should identify customer_lifetime_value in domains
    let clv_path = FactPath::local("customer_lifetime_value".to_string());
    assert!(
        solutions.domains.iter().any(|d| d.contains_key(&clv_path)),
        "customer_lifetime_value should be in domains"
    );
}

/// Test cross-spec inversion - Multi-level inheritance
#[test]
fn cross_spec_multi_level() {
    let global_spec = r#"
        spec global
        fact base_rate: 0.10
    "#;

    let regional_spec = r#"
        spec regional
        fact global_config: spec global
        fact regional_multiplier: 1.5

        rule effective_rate: global_config.base_rate * regional_multiplier
    "#;

    let transaction_spec = r#"
        spec transaction
        fact regional: spec regional
        fact amount: [number]

        rule fee: amount * regional.effective_rate
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, global_spec, "global").unwrap();
    add_lemma_code_blocking(&mut engine, regional_spec, "regional").unwrap();
    add_lemma_code_blocking(&mut engine, transaction_spec, "transaction").unwrap();

    let mut given = HashMap::new();
    given.insert(
        "regional.global_config.base_rate".to_string(),
        "0.10".to_string(),
    );
    given.insert(
        "regional.regional_multiplier".to_string(),
        "1.5".to_string(),
    );

    // Question: "What amount gives $15 fee?"
    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "transaction",
            &now,
            "fee",
            Target::value(LiteralValue::number(15.into())),
            given,
        )
        .expect("should invert successfully");

    // Should solve: amount = 15 / 0.15 = 100
    let amount_path = FactPath::local("amount".to_string());
    assert!(
        solutions.domains.iter().all(|d| d.is_empty())
            || solutions
                .domains
                .iter()
                .any(|d| d.contains_key(&amount_path)),
        "amount should be in domains or fully solved"
    );
}

/// Test cross-spec with piecewise rules
#[test]
fn cross_spec_piecewise() {
    let base_spec = r#"
        spec base
        fact tier: "gold"

        rule discount_rate: 0%
          unless tier is "silver" then 10%
          unless tier is "gold" then 20%
          unless tier is "platinum" then 30%
    "#;

    let pricing_spec = r#"
        spec pricing
        fact customer: spec base
        fact subtotal: [number]

        rule discount: subtotal * customer.discount_rate
        rule total: subtotal - discount
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, base_spec, "base").unwrap();
    add_lemma_code_blocking(&mut engine, pricing_spec, "pricing").unwrap();

    let mut given = HashMap::new();
    given.insert("subtotal".to_string(), "100".to_string());

    // Question: "What tier gives $80 total?" (i.e., 20% discount)
    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "pricing",
            &now,
            "total",
            Target::value(LiteralValue::number(80.into())),
            given,
        )
        .expect("should invert successfully");

    // Should identify tier as the free variable (or solve it exactly)
    assert!(!solutions.is_empty(), "should have branches");
    // Either tier is free, or it was fully solved (no free vars means solved)
    let has_tier = solutions
        .domains
        .iter()
        .any(|d| d.keys().any(|v| v.fact.contains("tier")));
    let fully_solved = solutions.domains.iter().all(|d| d.is_empty());
    assert!(
        has_tier || fully_solved,
        "tier should be involved or fully solved"
    );
}

/// Test Complex Boolean Expression with NOT and AND
#[test]
fn complex_boolean_not_and_combination() {
    let code = r#"
        spec shipping
        fact is_domestic: [boolean]
        fact has_po_box: [boolean]
        fact is_oversized: [boolean]

        rule can_ship: true
          unless not is_domestic and is_oversized
            then veto "Cannot ship oversized internationally"
          unless is_domestic and has_po_box and is_oversized
            then veto "Cannot ship oversized to PO box"
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();
    let now = DateTimeValue::now();

    let solutions = engine
        .invert(
            "shipping",
            &now,
            "can_ship",
            Target::any_veto(),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should have solutions
    assert!(!solutions.is_empty(), "should have solutions");

    // Should track all boolean variables in domains
    assert!(
        solutions.domains.iter().any(|d| d.keys().any(|k| {
            k.fact.contains("is_domestic")
                || k.fact.contains("has_po_box")
                || k.fact.contains("is_oversized")
        })),
        "should track condition variables"
    );
}

/// Test TargetOp::Neq (Not Equal)
#[test]
fn target_operator_not_equal() {
    let code = r#"
        spec validation
        fact status: [text]

        rule is_complete: status is "complete"
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Question: "What status values are NOT complete?"
    let now = DateTimeValue::now();
    let result = engine.invert(
        "validation",
        &now,
        "is_complete",
        Target::with_op(
            TargetOp::Neq,
            lemma::OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
        ),
        HashMap::new(),
    );

    let solutions = result.expect("Neq should be supported");
    let status_path = FactPath::local("status".to_string());
    assert!(
        solutions
            .domains
            .iter()
            .any(|d| d.contains_key(&status_path)),
        "status should be in domains"
    );
}
