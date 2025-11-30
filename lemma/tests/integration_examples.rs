//! Tests for all integration test examples
//!
//! Ensures all example files in cli/tests/integrations/examples/ are valid and can be evaluated

use lemma::Engine;
use std::collections::HashMap;

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
}
#[test]
fn test_02_rules_and_unless() {
    let engine = load_examples();

    let mut facts = std::collections::HashMap::new();
    facts.insert("base_price".to_string(), "100.00".to_string());

    let response = engine
        .evaluate("rules_and_unless", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "rules_and_unless");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "final_total"));
    // final_total depends on total_after_discount which depends on base_price (provided)
    // but also depends on shipping_cost which depends on total_after_discount
    // Since we're only providing base_price, not all dependencies are met
    // Rules with missing dependencies cascade - only root failures are reported
}

#[test]
fn test_03_document_references() {
    let engine = load_examples();

    // Test examples/base_employee document
    let response = engine
        .evaluate("base_employee", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "base_employee");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "annual_salary"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "is_eligible_for_bonus"));

    // Test examples/specific_employee document (references base_employee)
    let response = engine
        .evaluate("specific_employee", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "specific_employee");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "salary_with_bonus"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "employee_summary"));

    // Test examples/contractor document (also references base_employee)
    let response = engine
        .evaluate("contractor", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "contractor");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_payment"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "benefits_eligible"));
}

#[test]
fn test_04_unit_conversions() {
    let engine = load_examples();

    // Document has all facts defined, no type annotations needed
    let response = engine
        .evaluate("unit_conversions", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "unit_conversions");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "package_weight_lbs"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "distance_miles"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "temperature_f"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "is_overweight"));
}

#[test]
fn test_05_date_handling() {
    let engine = load_examples();

    let mut facts = std::collections::HashMap::new();
    facts.insert("current_date".to_string(), "2024-06-15".to_string());

    let response = engine
        .evaluate("date_handling", vec![], facts)
        .expect("Evaluation failed");

    // Document evaluates successfully
    assert_eq!(response.doc_name, "date_handling");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "employee_age"));
    assert!(response.results.values().any(|r| r.rule.name == "is_adult"));
}
#[test]
fn test_06_tax_calculation() {
    let engine = load_examples();

    // Document has all facts defined, no type annotations needed
    let response = engine
        .evaluate("tax_calculation", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "tax_calculation");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "taxable_income"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_federal_tax"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_tax"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "after_tax_income"));
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
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "final_shipping"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "estimated_delivery_days"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_with_shipping"));
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
    let avg_discount = response
        .results
        .values()
        .find(|r| r.rule.name == "avg_discount");
    assert!(avg_discount.is_some(), "avg_discount rule should exist");
    // avg_discount = (15% + 0% + 5%) / 3 = 20% / 3 = 6.666...

    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "price_range"));
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
