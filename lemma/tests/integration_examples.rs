//! Tests for all integration test examples
//!
//! Ensures all example files in cli/tests/integrations/examples/ are valid and can be evaluated
//! Verifies that rules calculate correct values, not just that they exist

use lemma::{Engine, LiteralValue, OperationResult};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn load_examples() -> Engine {
    let mut engine = Engine::new();

    // Load all example files - paths relative to lemma/ crate
    let examples = [
        "../cli/tests/integrations/examples/01_simple_facts.lemma",
        "../cli/tests/integrations/examples/02_rules_and_unless.lemma",
        "../cli/tests/integrations/examples/03_document_references.lemma",
        "../cli/tests/integrations/examples/04_unit_conversions.lemma",
        "../cli/tests/integrations/examples/05_date_handling.lemma",
        "../cli/tests/integrations/examples/06_tax_calculation.lemma",
        "../cli/tests/integrations/examples/07_shipping_policy.lemma",
        "../cli/tests/integrations/examples/08_rule_references.lemma",
        "../cli/tests/integrations/examples/09_stress_test.lemma",
        "../cli/tests/integrations/examples/10_compensation_policy.lemma",
        "../cli/tests/integrations/examples/11_document_composition.lemma",
    ];

    for path in examples {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
        engine
            .add_lemma_code(&content, path)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path, e));
    }

    engine
}

#[test]
fn test_01_simple_facts() {
    let engine = load_examples();

    // Document has only facts, no rules - just verify it loads without errors
    let response = engine
        .evaluate("simple_facts", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "simple_facts");
    // No rules in this document, just facts
    assert_eq!(response.results.len(), 0);
    
    // Verify facts are loaded and exposed in the response
    // This is important for API consumers who need to see fact values
    assert!(
        !response.facts.is_empty(),
        "Facts should be exposed in response.facts. Got empty facts array. This indicates facts are not being properly exposed in the response structure."
    );
}
#[test]
fn test_02_rules_and_unless() {
    let engine = load_examples();

    // Test with missing facts - should fail gracefully
    let mut facts = std::collections::HashMap::new();
    facts.insert("base_price".to_string(), "100.00".to_string());

    let response = engine
        .evaluate("rules_and_unless", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "rules_and_unless");
    let final_total_result = response
        .results
        .values()
        .find(|r| r.rule.name == "final_total");

    // final_total depends on quantity which is missing, so it should fail
    if let Some(result) = final_total_result {
        match &result.result {
            OperationResult::Veto(_) => {
                // Expected - missing quantity fact causes failure
            }
            other => panic!(
                "final_total should fail with missing quantity, got: {:?}",
                other
            ),
        }
    }

    // Test with all required facts provided
    let mut all_facts = std::collections::HashMap::new();
    all_facts.insert("base_price".to_string(), "100.00".to_string());
    all_facts.insert("quantity".to_string(), "15".to_string());

    let response_complete = engine
        .evaluate("rules_and_unless", vec![], all_facts)
        .expect("Evaluation failed");

    // Verify calculations are correct
    // base_price = 100, quantity = 15
    // total_before_discount = 100 * 15 = 1500
    // discount_percentage = 10% (quantity >= 10, first unless matches)
    // total_after_discount = 1500 - (1500 * 0.10) = 1350
    // shipping_cost: 1350 >= 200, so second unless matches -> 0
    // final_total = 1350 + 0 = 1350

    let final_total_complete = response_complete
        .results
        .values()
        .find(|r| r.rule.name == "final_total")
        .expect("final_total rule should exist");

    match &final_total_complete.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            // Expected calculation:
            // base_price = 100, quantity = 15
            // total_before_discount = 100 * 15 = 1500
            // discount_percentage = 10% (quantity >= 10, first unless matches)
            // total_after_discount = 1500 - (1500 * 0.10) = 1350
            // shipping_cost: 1350 >= 200, so second unless matches -> 0
            // final_total = 1350 + 0 = 1350
            let expected = Decimal::from_str("1350").unwrap();
            assert_eq!(
                *n,
                expected,
                "final_total calculation is incorrect. Expected 1350 (base_price=100, quantity=15, discount=10%, shipping=0), got {}",
                n
            );
        }
        other => panic!(
            "final_total should have a value when all facts provided, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_03_document_references() {
    let engine = load_examples();

    // Test examples/base_employee document
    let response = engine
        .evaluate("base_employee", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "base_employee");

    // Verify annual_salary = monthly_salary * 12 = 5000 * 12 = 60000
    let annual_salary_result = response
        .results
        .values()
        .find(|r| r.rule.name == "annual_salary")
        .expect("annual_salary rule should exist");

    match &annual_salary_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("60000").unwrap());
        }
        other => panic!("annual_salary should be 60000, got: {:?}", other),
    }

    // Verify is_eligible_for_bonus = false (employment_duration = 0 years < 1 year)
    let bonus_eligible_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_eligible_for_bonus")
        .expect("is_eligible_for_bonus rule should exist");

    match &bonus_eligible_result.result {
        OperationResult::Value(LiteralValue::Boolean(b)) => {
            assert!(!bool::from(b), "is_eligible_for_bonus should be false");
        }
        other => panic!("is_eligible_for_bonus should be boolean false, got: {:?}", other),
    }

    // Test examples/specific_employee document (references base_employee)
    let response = engine
        .evaluate("specific_employee", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "specific_employee");

    // Verify salary_with_bonus = employee.annual_salary? * 1.1
    // employee.monthly_salary = 7500 (overridden), so annual = 7500 * 12 = 90000
    // employment_duration = 3 years >= 1 year, so is_eligible_for_bonus = true
    // salary_with_bonus = 90000 * 1.1 = 99000
    let salary_with_bonus_result = response
        .results
        .values()
        .find(|r| r.rule.name == "salary_with_bonus")
        .expect("salary_with_bonus rule should exist");

    match &salary_with_bonus_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("99000").unwrap());
        }
        other => panic!("salary_with_bonus should be 99000, got: {:?}", other),
    }

    // Verify employee_summary = employee.name = "Alice Smith" (overridden)
    let employee_summary_result = response
        .results
        .values()
        .find(|r| r.rule.name == "employee_summary")
        .expect("employee_summary rule should exist");

    match &employee_summary_result.result {
        OperationResult::Value(LiteralValue::Text(text)) => {
            assert_eq!(text, "Alice Smith");
        }
        other => panic!("employee_summary should be 'Alice Smith', got: {:?}", other),
    }

    // Test examples/contractor document (also references base_employee)
    let response = engine
        .evaluate("contractor", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "contractor");

    // Verify total_payment = hourly_rate * hours_worked = 85 * 120 hours = 10200 hours (Duration unit)
    let total_payment_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total_payment")
        .expect("total_payment rule should exist");

    match &total_payment_result.result {
        OperationResult::Value(LiteralValue::Unit(lemma::NumericUnit::Duration(value, unit))) => {
            assert_eq!(*unit, lemma::DurationUnit::Hour, "total_payment unit should be hours");
            assert_eq!(*value, Decimal::from_str("10200").unwrap());
        }
        other => panic!("total_payment should be 10200 hours (Duration unit), got: {:?}", other),
    }

    // Verify benefits_eligible = true (annual_hours = 120 * 12 = 1440 > 1000)
    let benefits_eligible_result = response
        .results
        .values()
        .find(|r| r.rule.name == "benefits_eligible")
        .expect("benefits_eligible rule should exist");

    match &benefits_eligible_result.result {
        OperationResult::Value(LiteralValue::Boolean(b)) => {
            assert!(bool::from(b), "benefits_eligible should be true");
        }
        other => panic!("benefits_eligible should be boolean true, got: {:?}", other),
    }
}

#[test]
fn test_04_unit_conversions() {
    let engine = load_examples();

    // Document has all facts defined, no type annotations needed
    let response = engine
        .evaluate("unit_conversions", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "unit_conversions");

    // Verify package_weight_lbs: 25 kg ≈ 55.1156 lbs
    let package_weight_lbs_result = response
        .results
        .values()
        .find(|r| r.rule.name == "package_weight_lbs")
        .expect("package_weight_lbs rule should exist");

    match &package_weight_lbs_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            let expected = Decimal::from_str("55.1156").unwrap();
            let diff = (*n - expected).abs();
            assert!(
                diff < Decimal::from_str("0.1").unwrap(),
                "package_weight_lbs should be approximately 55.12 lbs (25 kg), got {}",
                n
            );
        }
        other => panic!("package_weight_lbs should be a number, got: {:?}", other),
    }

    // Verify distance_miles: 100 km ≈ 62.1371 miles
    let distance_miles_result = response
        .results
        .values()
        .find(|r| r.rule.name == "distance_miles")
        .expect("distance_miles rule should exist");

    match &distance_miles_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            let expected = Decimal::from_str("62.1371").unwrap();
            let diff = (*n - expected).abs();
            assert!(
                diff < Decimal::from_str("0.1").unwrap(),
                "distance_miles should be approximately 62.14 miles (100 km), got {}",
                n
            );
        }
        other => panic!("distance_miles should be a number, got: {:?}", other),
    }

    // Verify temperature_f: 20°C = 68°F
    let temperature_f_result = response
        .results
        .values()
        .find(|r| r.rule.name == "temperature_f")
        .expect("temperature_f rule should exist");

    match &temperature_f_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            let expected = Decimal::from_str("68").unwrap();
            let diff = (*n - expected).abs();
            assert!(
                diff < Decimal::from_str("0.1").unwrap(),
                "temperature_f should be 68°F (20°C), got {}",
                n
            );
        }
        other => panic!("temperature_f should be a number, got: {:?}", other),
    }

    // Verify is_overweight: 25 kg in pounds = ~55.12 lbs > 50 lbs, so should be true
    let is_overweight_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_overweight")
        .expect("is_overweight rule should exist");

    match &is_overweight_result.result {
        OperationResult::Value(LiteralValue::Boolean(b)) => {
            assert!(bool::from(b), "is_overweight should be true (25 kg > 50 lbs limit)");
        }
        other => panic!("is_overweight should be boolean true, got: {:?}", other),
    }
}

#[test]
fn test_05_date_handling() {
    let engine = load_examples();

    let mut facts = std::collections::HashMap::new();
    facts.insert("current_date".to_string(), "2024-06-15".to_string());

    let response = engine
        .evaluate("date_handling", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "date_handling");

    // Verify employee_age: (2024-06-15 - 1990-05-20) in years
    // Note: "in years" conversion may not be supported (calendar units), so this might fail
    let employee_age_result = response
        .results
        .values()
        .find(|r| r.rule.name == "employee_age")
        .expect("employee_age rule should exist");

    match &employee_age_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            // 2024-06-15 - 1990-05-20 = approximately 34 years
            let expected = Decimal::from_str("34").unwrap();
            let diff = (*n - expected).abs();
            assert!(
                diff < Decimal::from_str("1").unwrap(),
                "employee_age should be approximately 34 years, got {}",
                n
            );
        }
        OperationResult::Veto(msg) => {
            panic!(
                "employee_age calculation failed: Calendar unit conversion (date difference 'in years') is not working. Got veto: {:?}. This feature needs to be implemented or the test needs to be updated to reflect the actual limitation.",
                msg
            );
        }
        other => panic!("employee_age should be a number, got: {:?}", other),
    }

    // Verify is_adult: employee_age >= 18, so should be true
    // Note: If employee_age fails (calendar unit conversion), is_adult will also fail
    let is_adult_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_adult")
        .expect("is_adult rule should exist");

    match &is_adult_result.result {
        OperationResult::Value(LiteralValue::Boolean(b)) => {
            assert!(bool::from(b), "is_adult should be true (age >= 18)");
        }
        OperationResult::Veto(msg) => {
            panic!(
                "is_adult calculation failed: This depends on employee_age which failed due to calendar unit conversion. Got veto: {:?}. Fix employee_age first.",
                msg
            );
        }
        other => panic!("is_adult should be boolean, got: {:?}", other),
    }
}
#[test]
fn test_06_tax_calculation() {
    let engine = load_examples();

    // Document has all facts defined, no type annotations needed
    let response = engine
        .evaluate("tax_calculation", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "tax_calculation");

    // Verify taxable_income = income - deductions = 85000 - 12000 = 73000
    let taxable_income_result = response
        .results
        .values()
        .find(|r| r.rule.name == "taxable_income")
        .expect("taxable_income rule should exist");

    match &taxable_income_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("73000").unwrap());
        }
        other => panic!("taxable_income should be 73000, got: {:?}", other),
    }

    // Verify total_federal_tax exists and is a positive number
    let total_federal_tax_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total_federal_tax")
        .expect("total_federal_tax rule should exist");

    match &total_federal_tax_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            assert!(*n > Decimal::from_str("0").unwrap(), "total_federal_tax should be positive");
            // For taxable_income = 73000, federal tax should be in bracket 3
            // Rough calculation: should be several thousand dollars
            assert!(*n > Decimal::from_str("1000").unwrap(), "total_federal_tax should be substantial");
        }
        other => panic!("total_federal_tax should be a number, got: {:?}", other),
    }

    // Verify total_tax exists and is positive
    let total_tax_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total_tax")
        .expect("total_tax rule should exist");

    match &total_tax_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            assert!(*n > Decimal::from_str("0").unwrap(), "total_tax should be positive");
        }
        other => panic!("total_tax should be a number, got: {:?}", other),
    }

    // Verify after_tax_income = income - total_tax < income
    let after_tax_income_result = response
        .results
        .values()
        .find(|r| r.rule.name == "after_tax_income")
        .expect("after_tax_income rule should exist");

    match &after_tax_income_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            let income = Decimal::from_str("85000").unwrap();
            assert!(*n < income, "after_tax_income should be less than income");
            assert!(*n > Decimal::from_str("60000").unwrap(), "after_tax_income should be reasonable");
        }
        other => panic!("after_tax_income should be a number, got: {:?}", other),
    }
}

#[test]
fn test_07_shipping_policy() {
    let engine = load_examples();

    let mut facts = std::collections::HashMap::new();
    facts.insert("order_total".to_string(), "75.00".to_string());
    facts.insert("item_weight".to_string(), "8 kilograms".to_string());
    facts.insert("destination_country".to_string(), "US".to_string());
    facts.insert("destination_state".to_string(), "CA".to_string());
    facts.insert("is_po_box".to_string(), "false".to_string());
    facts.insert("is_expedited".to_string(), "false".to_string());
    facts.insert("is_hazardous".to_string(), "false".to_string());

    let response = engine
        .evaluate("shipping_policy", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "shipping_policy");

    // Verify final_shipping calculation:
    // base_shipping_rate = 12.99 (US)
    // weight_surcharge = 7.50 (8 kg > 5 kg)
    // customer_discount = 20% (gold tier)
    // shipping_before_discount = 12.99 + 7.50 = 20.49
    // shipping_discount_amount = 20.49 * 0.20 = 4.098
    // final_shipping = 20.49 - 4.098 = 16.392
    let final_shipping_result = response
        .results
        .values()
        .find(|r| r.rule.name == "final_shipping")
        .expect("final_shipping rule should exist");

    match &final_shipping_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            let expected = Decimal::from_str("16.392").unwrap();
            let diff = (*n - expected).abs();
            assert!(
                diff < Decimal::from_str("0.01").unwrap(),
                "final_shipping should be approximately 16.39, got {}",
                n
            );
        }
        other => panic!("final_shipping should be a number, got: {:?}", other),
    }

    // Verify estimated_delivery_days = 5 days (US destination)
    // Note: This returns a Duration unit, not a plain number
    let estimated_delivery_result = response
        .results
        .values()
        .find(|r| r.rule.name == "estimated_delivery_days")
        .expect("estimated_delivery_days rule should exist");

    match &estimated_delivery_result.result {
        OperationResult::Value(LiteralValue::Unit(lemma::NumericUnit::Duration(value, unit))) => {
            // Should be 5 days for US
            assert_eq!(*unit, lemma::DurationUnit::Day, "estimated_delivery_days unit should be days");
            let expected = Decimal::from_str("5").unwrap();
            let diff = (*value - expected).abs();
            assert!(
                diff < Decimal::from_str("0.1").unwrap(),
                "estimated_delivery_days should be 5 days for US, got {}",
                value
            );
        }
        other => panic!("estimated_delivery_days should be a Duration unit, got: {:?}", other),
    }

    // Verify total_with_shipping = order_total + final_shipping = 75.00 + 16.392 = 91.392
    let total_with_shipping_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total_with_shipping")
        .expect("total_with_shipping rule should exist");

    match &total_with_shipping_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            let expected = Decimal::from_str("91.392").unwrap();
            let diff = (*n - expected).abs();
            assert!(
                diff < Decimal::from_str("0.01").unwrap(),
                "total_with_shipping should be approximately 91.39, got {}",
                n
            );
        }
        other => panic!("total_with_shipping should be a number, got: {:?}", other),
    }
}

#[test]
fn test_08_rule_references() {
    let engine = load_examples();

    // Test examples/rule_references document
    let response = engine
        .evaluate("rule_references", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "rule_references");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "can_drive_legally"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "driving_status"));

    // Test examples/eligibility_check document (also in the same file)
    let response = engine
        .evaluate("eligibility_check", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "eligibility_check");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "can_travel_internationally"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "eligibility_message"));
}

#[test]
fn test_09_stress_test() {
    let engine = load_examples();

    let mut facts = std::collections::HashMap::new();
    facts.insert("base_price".to_string(), "100.00".to_string());
    facts.insert("quantity".to_string(), "50".to_string());
    facts.insert("customer_tier".to_string(), "premium".to_string());
    facts.insert("loyalty_points".to_string(), "5000".to_string());
    facts.insert("package_weight".to_string(), "25".to_string());
    facts.insert("delivery_distance".to_string(), "300".to_string());
    facts.insert("is_express".to_string(), "false".to_string());
    facts.insert("is_fragile".to_string(), "false".to_string());
    facts.insert("payment_method".to_string(), "credit".to_string());

    let response = engine
        .evaluate("stress_test", vec![], facts)
        .expect("Evaluation should succeed");

    assert_eq!(response.doc_name, "stress_test");
    assert!(!response.results.is_empty());
}

#[test]
fn test_09_stress_test_config() {
    let engine = load_examples();

    // Test the config document (has all facts defined)
    let response = engine
        .evaluate("stress_test_config", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "stress_test_config");
    // Config doc only has facts, no rules to check
    assert_eq!(response.results.len(), 0, "Config document should have no rules");
    
    // Verify facts are loaded and exposed in the response
    assert!(
        !response.facts.is_empty(),
        "Facts should be exposed in response.facts for stress_test_config. Got empty facts array. This indicates facts are not being properly exposed in the response structure."
    );
}

#[test]
fn test_09_stress_test_extended() {
    let engine = load_examples();

    let mut facts = std::collections::HashMap::new();
    facts.insert("order.base_price".to_string(), "100.00".to_string());
    facts.insert("order.quantity".to_string(), "100".to_string());
    facts.insert("order.customer_tier".to_string(), "vip".to_string());
    facts.insert("order.loyalty_points".to_string(), "10000".to_string());
    facts.insert("order.package_weight".to_string(), "30".to_string());
    facts.insert("order.delivery_distance".to_string(), "250".to_string());
    facts.insert("order.is_express".to_string(), "true".to_string());
    facts.insert("order.is_fragile".to_string(), "true".to_string());
    facts.insert("order.payment_method".to_string(), "debit".to_string());

    let response = engine
        .evaluate("stress_test_extended", vec![], facts)
        .expect("Cross-document rule references now work correctly");

    assert_eq!(response.doc_name, "stress_test_extended");
    assert!(!response.results.is_empty());
}

#[test]
fn test_10_compensation_policy() {
    let engine = load_examples();

    // Test base_policy document
    let response = engine
        .evaluate("compensation/base_policy", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "compensation/base_policy");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "annual_health_cost"));

    // Test engineering_dept document (has all facts defined)
    let response = engine
        .evaluate("compensation/engineering_dept", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "compensation/engineering_dept");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_package"));

    // Test senior_engineer document - now works after fixing cross-document rule reference bugs!
    let response = engine
        .evaluate("compensation/senior_engineer", vec![], HashMap::new())
        .unwrap();
    assert_eq!(response.doc_name, "compensation/senior_engineer");
    assert!(!response.results.is_empty());

    // Test principal_engineer document - now works after fixing cross-document rule reference bugs!
    let response = engine
        .evaluate("compensation/principal_engineer", vec![], HashMap::new())
        .unwrap();
    assert_eq!(response.doc_name, "compensation/principal_engineer");
    assert!(!response.results.is_empty());
}

#[test]
fn test_11_document_composition() {
    let engine = load_examples();

    // Test base pricing configuration
    let response = engine
        .evaluate("pricing/base_config", vec![], HashMap::new())
        .expect("Failed to evaluate base_config");
    assert_eq!(response.doc_name, "pricing/base_config");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "final_price"));

    // Test wholesale pricing with overrides
    let response = engine
        .evaluate("pricing/wholesale", vec![], HashMap::new())
        .expect("Failed to evaluate wholesale");
    assert_eq!(response.doc_name, "pricing/wholesale");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "wholesale_final"));

    // Test multi-level nested references - now works correctly!
    let response = engine
        .evaluate("order/wholesale_order", vec![], HashMap::new())
        .expect("Cross-document rule references now work correctly");
    assert_eq!(response.doc_name, "order/wholesale_order");
    let order_total = response
        .results
        .values()
        .find(|r| r.rule.name == "order_total");
    assert!(order_total.is_some(), "order_total rule should exist");
    assert!(
        order_total.unwrap().result.value().is_some(),
        "order_total should have a value"
    );

    // Test comparison document with multiple references
    let response = engine
        .evaluate("order/comparison", vec![], HashMap::new())
        .expect("Evaluation should succeed (but rules will veto)");
    assert_eq!(response.doc_name, "order/comparison");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "wholesale_total"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "retail_total"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "price_difference"));

    // Test deep nested overrides
    let response = engine
        .evaluate("order/custom_wholesale", vec![], HashMap::new())
        .expect("Failed to evaluate custom_wholesale");
    assert_eq!(response.doc_name, "order/custom_wholesale");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "custom_total"));

    // Test multiple independent references
    let response = engine
        .evaluate("complex/multi_reference", vec![], HashMap::new())
        .expect("Failed to evaluate multi_reference");
    assert_eq!(response.doc_name, "complex/multi_reference");

    // Check avg_discount calculation works (tests percentage arithmetic)
    // avg_discount = (wholesale_config.standard_discount + retail_config.standard_discount + base_config.standard_discount) / 3
    // = (15% + 0% + 5%) / 3 = 20% / 3 = 6.666...%
    let avg_discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "avg_discount")
        .expect("avg_discount rule should exist");

    match &avg_discount_result.result {
        OperationResult::Value(LiteralValue::Percentage(n)) => {
            // 20% / 3 = 6.666...%
            let expected = Decimal::from_str("6.666666666666666666666666667").unwrap();
            let diff = (*n - expected).abs();
            assert!(
                diff < Decimal::from_str("0.01").unwrap(),
                "avg_discount should be approximately 6.67%, got {}%",
                n
            );
        }
        other => panic!("avg_discount should be a percentage, got: {:?}", other),
    }

    // Verify price_range exists and is a positive number
    let price_range_result = response
        .results
        .values()
        .find(|r| r.rule.name == "price_range")
        .expect("price_range rule should exist");

    match &price_range_result.result {
        OperationResult::Value(LiteralValue::Number(n)) => {
            assert!(*n > Decimal::from_str("0").unwrap(), "price_range should be positive");
        }
        other => panic!("price_range should be a number, got: {:?}", other),
    }
}

#[test]
fn test_all_examples_parse() {
    // This test just ensures all examples can be loaded without errors
    let engine = load_examples();

    // Verify all documents are loaded
    let docs = engine.list_documents();

    // Just verify we have a reasonable number of documents loaded
    assert!(
        docs.len() >= 10,
        "Expected at least 10 documents, found {}. Available: {:?}",
        docs.len(),
        docs
    );

    // Verify some key documents exist
    let key_docs = vec![
        "simple_facts",
        "rules_and_unless",
        "stress_test",
        "stress_test_extended",
    ];

    for expected in key_docs {
        assert!(
            docs.contains(&expected.to_string()),
            "Expected document '{}' not found. Available: {:?}",
            expected,
            docs
        );
    }
}
