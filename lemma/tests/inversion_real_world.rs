use lemma::{Engine, LiteralValue, MoneyUnit, NumericUnit, Target, TargetOp};
use rust_decimal::Decimal;
use std::collections::HashMap;

fn usd(amount: i64) -> LiteralValue {
    LiteralValue::Unit(NumericUnit::Money(Decimal::from(amount), MoneyUnit::Usd))
}

/// Use Case 1: Tax Planning - "What income do I need to have X amount after tax?"
#[test]
fn tax_calculation_inversion_after_tax_target() {
    let code = r#"
        doc tax_planning
        fact income = [money]
        fact deductions = 12000 USD

        rule taxable_income = income - deductions
          unless income < deductions then 0

        rule tax_rate = 0.10
          unless taxable_income? > 50000 then 0.22
          unless taxable_income? > 100000 then 0.24

        rule tax_owed = taxable_income? * tax_rate?

        rule after_tax_income = income - tax_owed?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Question: "What income do I need for $80,000 after tax?"
    let solutions = engine
        .invert(
            "tax_planning",
            "after_tax_income",
            Target::value(usd(80000)),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should return a relationship showing the dependency on income
    // Income should be a free variable
    assert!(
        solutions
            .iter()
            .flat_map(|r| r.keys())
            .any(|v| v.reference.join(".") == "income"),
        "income should be a free variable"
    );
}

/// Use Case 2: Pricing Strategy - "What order total gives free shipping?"
#[test]
fn shipping_policy_free_shipping_threshold() {
    let code = r#"
        doc ecommerce
        fact order_total = [money]
        fact destination_country = "US"

        rule base_shipping = 12.99 USD

        rule free_shipping_eligible = order_total >= 100 and destination_country is "US"

        rule final_shipping = base_shipping?
          unless free_shipping_eligible? then 0
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Question: "What order totals result in free shipping?"
    let solutions = engine
        .invert(
            "ecommerce",
            "final_shipping",
            Target::value(usd(0)),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should give us a piecewise relationship with the condition
    assert!(!solutions.is_empty(), "should have branches");
    // order_total should be a free variable
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "order_total"));
}

/// Use Case 3: Validation - "What weights cause shipping to veto?"
#[test]
fn shipping_policy_weight_restrictions() {
    let code = r#"
        doc shipping
        fact item_weight = [mass]

        rule weight_check = "ok"
          unless item_weight > 20 kilograms then veto "Too heavy for standard shipping"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Question: "What weights are invalid?"
    let solutions = engine
        .invert(
            "shipping",
            "weight_check",
            Target::any_veto(),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should give us the veto condition: weight > 20 kg
    assert!(!solutions.is_empty(), "should have veto branch");
    // Check that weight is tracked as a free variable
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "item_weight"));
}

/// Use Case 4: Salary Planning - "What base determines my total compensation?"
#[test]
fn compensation_total_package_inversion() {
    let code = r#"
        doc compensation
        fact base_salary = [money]

        rule stock_grant_percentage = 5%
        rule bonus_percentage = 15%

        rule stock_value = base_salary * stock_grant_percentage?
        rule bonus_value = base_salary * bonus_percentage?

        rule total_compensation = base_salary + stock_value? + bonus_value?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Question: "What base salary gives $150,000 total comp?"
    let solutions = engine
        .invert(
            "compensation",
            "total_compensation",
            Target::value(usd(150000)),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should solve algebraically: base = 150000 / 1.20
    assert!(!solutions.is_empty(), "Expected at least one solution");
}

/// Use Case 5: Performance Tiers - "What ratings get which bonus?"
#[test]
fn performance_bonus_tiers() {
    let code = r#"
        doc performance
        fact performance_rating = [number]
        fact base_salary = 100000 USD

        rule bonus_rate = 0%
          unless performance_rating >= 2.5 then 5%
          unless performance_rating >= 3.5 then 10%
          unless performance_rating >= 4.5 then 15%

        rule bonus_amount = base_salary * bonus_rate?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert("performance.base_salary".to_string(), usd(100000));

    // Question: "What ratings give $10,000 bonus?" (i.e., 10% of 100k)
    let solutions = engine
        .invert(
            "performance",
            "bonus_amount",
            Target::value(usd(10000)),
            given,
        )
        .expect("should invert successfully");

    // Should give piecewise with conditions showing which rating tier
    assert!(!solutions.is_empty(), "should have branches for tiers");
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "performance_rating"));
}

/// Use Case 6: Cost Analysis - "What discounts achieve target price?"
#[test]
fn shipping_discount_inversion() {
    let code = r#"
        doc shipping
        fact base_shipping = 50 USD
        fact customer_tier = "bronze"

        rule discount_rate = 0%
          unless customer_tier is "silver" then 10%
          unless customer_tier is "gold" then 20%
          unless customer_tier is "platinum" then 30%

        rule discount_amount = base_shipping * discount_rate?
        rule final_shipping = base_shipping - discount_amount?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert("shipping.base_shipping".to_string(), usd(50));

    // Question: "What tiers give $35 final shipping?"
    let solutions = engine
        .invert("shipping", "final_shipping", Target::value(usd(35)), given)
        .expect("should invert successfully");

    // Should be piecewise with customer_tier conditions
    assert!(!solutions.is_empty());
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "customer_tier"));
}

/// Use Case 7: Budget Constraints - "What salaries stay under budget?"
#[test]
fn salary_budget_constraint_inequality() {
    let code = r#"
        doc budget
        fact base_salary = [money]
        fact bonus_multiplier = 1.2

        rule total_cost = base_salary * bonus_multiplier
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Question: "What salaries keep total cost under $100k?"
    let solutions = engine
        .invert(
            "budget",
            "total_cost",
            Target::with_op(TargetOp::Lt, lemma::OperationResult::Value(usd(100000))),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should work with inequality
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "base_salary"));
}

/// Use Case 8: Complex Dependencies - Multiple rule references
#[test]
fn complex_rule_dependencies() {
    let code = r#"
        doc complex
        fact base = [money]
        fact multiplier = 1.5

        rule component_a = base * multiplier
        rule component_b = component_a? * 0.8
        rule component_c = component_b? + 1000

        rule total = component_c?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Question: "What base gives total of $50,000?"
    let solutions = engine
        .invert(
            "complex",
            "total",
            Target::value(usd(50000)),
            HashMap::new(),
        )
        .expect("should invert successfully");

    // Should track transitive dependencies through rule chain
    // base -> component_a -> component_b -> component_c -> total
    // Should track dependencies
    assert!(
        solutions
            .iter()
            .flat_map(|r| r.keys())
            .any(|v| v.reference.join(".") == "base")
            || solutions
                .iter()
                .flat_map(|r| r.keys())
                .any(|v| v.reference.join(".") == "multiplier"),
        "should track dependencies"
    );
}

/// Use Case 9: Conditional Vetos - Multiple veto conditions
#[test]
fn multiple_veto_conditions() {
    let code = r#"
        doc shipping
        fact is_po_box = [boolean]
        fact is_hazardous = [boolean]
        fact destination_state = "CA"

        rule can_ship = true
          unless is_po_box and is_hazardous then veto "Cannot ship hazardous to PO box"
          unless destination_state is "AK" or destination_state is "HI"
            then veto "No shipping to Alaska/Hawaii"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Question: "What conditions trigger any veto?"
    let solutions = engine
        .invert("shipping", "can_ship", Target::any_veto(), HashMap::new())
        .expect("should invert successfully");

    // Should have solution solutions for veto conditions
    assert!(!solutions.is_empty(), "Expected at least one solution");
}

/// Use Case 10: Given Facts Constraint - Partial information
#[test]
fn inversion_with_given_facts() {
    let code = r#"
        doc pricing
        fact base_price = [money]
        fact quantity = [number]
        fact tax_rate = 0.08

        rule subtotal = base_price * quantity
        rule tax = subtotal? * tax_rate
        rule total = subtotal? + tax?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let mut given = HashMap::new();
    given.insert(
        "pricing.quantity".to_string(),
        LiteralValue::Number(Decimal::from(10)),
    );
    given.insert(
        "pricing.tax_rate".to_string(),
        LiteralValue::Number(Decimal::from_str_exact("0.08").unwrap()),
    );

    // Question: "Given quantity=10 and tax_rate=0.08, what base_price gives total=$108?"
    let solutions = engine
        .invert("pricing", "total", Target::value(usd(108)), given)
        .expect("should invert successfully");

    // Should solve for base_price: 108 / 10.8 = 10
    // Check if base_price is the only free variable (quantity and tax_rate are given)
    assert!(!solutions.is_empty(), "Expected at least one solution");

    let has_base_price = solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "base_price");
    let has_quantity = solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "quantity");

    assert!(has_base_price, "Should reference base_price");
    assert!(
        !has_quantity,
        "quantity should not be a free variable (it was given)"
    );
}
