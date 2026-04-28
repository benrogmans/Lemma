//! Tests for all integration test examples
//!
//! Ensures all example files in cli/tests/integrations/examples/ are valid and can be evaluated

use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn load_examples() -> Engine {
    let mut engine = Engine::new();

    // Load all example files - paths relative to lemma/ crate
    let examples = [
        "../cli/tests/integrations/examples/01_simple_data.lemma",
        "../cli/tests/integrations/examples/02_rules_and_unless.lemma",
        "../cli/tests/integrations/examples/03_spec_references.lemma",
        "../cli/tests/integrations/examples/04_unit_conversions.lemma",
        "../cli/tests/integrations/examples/05_date_handling.lemma",
        "../cli/tests/integrations/examples/06_tax_calculation.lemma",
        "../cli/tests/integrations/examples/07_shipping_policy.lemma",
        "../cli/tests/integrations/examples/08_rule_references.lemma",
        "../cli/tests/integrations/examples/09_stress_test.lemma",
        "../cli/tests/integrations/examples/10_compensation_policy.lemma",
        "../cli/tests/integrations/examples/11_spec_composition.lemma",
    ];

    for path in examples {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
        engine
            .load(&content, lemma::SourceType::Labeled(path))
            .unwrap_or_else(|errs| {
                panic!(
                    "Failed to parse {}: {}",
                    path,
                    errs.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            });
    }

    engine
}

#[test]
fn test_02_rules_and_unless() {
    let engine = load_examples();
    let now = DateTimeValue::now();

    let mut data = std::collections::HashMap::new();
    data.insert("base_price".to_string(), "100.00".to_string());
    data.insert("quantity".to_string(), "10".to_string());
    data.insert("is_premium".to_string(), "true".to_string());
    data.insert("customer_age".to_string(), "17".to_string());

    let response = engine
        .run("rules_and_unless", Some(&now), data, false)
        .expect("Evaluation failed");

    assert_eq!(response.spec_name, "rules_and_unless");

    let final_total = response.results.get("final_total").unwrap();
    match &final_total.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("800").unwrap()),
            other => panic!("Expected Number for final_total, got {:?}", other),
        },
        other => panic!("Expected Value for final_total, got {:?}", other),
    }

    let age_validation = response.results.get("age_validation").unwrap();
    assert_eq!(
        age_validation.result,
        lemma::OperationResult::Veto(lemma::VetoType::UserDefined {
            message: Some("Customer must be 18 or older".to_string()),
        })
    );
}

#[test]
fn test_03_spec_references() {
    let engine = load_examples();
    let now = DateTimeValue::now();

    // specific_employee (references base_employee)
    let response = engine
        .run("specific_employee", Some(&now), HashMap::new(), false)
        .expect("Evaluation failed");

    assert_eq!(response.spec_name, "specific_employee");
    let salary_with_bonus = response.results.get("salary_with_bonus").unwrap();
    match &salary_with_bonus.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("99000").unwrap()),
            other => panic!("Expected Number for salary_with_bonus, got {:?}", other),
        },
        other => panic!("Expected Value for salary_with_bonus, got {:?}", other),
    }

    let employee_summary = response.results.get("employee_summary").unwrap();
    match &employee_summary.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Text(s) => assert_eq!(s, "Alice Smith"),
            other => panic!("Expected Text for employee_summary, got {:?}", other),
        },
        other => panic!("Expected Value for employee_summary, got {:?}", other),
    }
}

#[test]
fn test_04_unit_conversions() {
    let engine = load_examples();
    let now = DateTimeValue::now();

    // Spec has all data defined, no type annotations needed
    let response = engine
        .run("unit_conversions", Some(&now), HashMap::new(), false)
        .expect("Evaluation failed");

    assert_eq!(response.spec_name, "unit_conversions");

    let duration_hours = response.results.get("duration_hours").unwrap();
    match &duration_hours.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Duration(v, unit) => {
                assert_eq!(*v, Decimal::from_str("1.5").unwrap());
                assert_eq!(*unit, lemma::SemanticDurationUnit::Hour);
            }
            other => panic!("Expected Duration for duration_hours, got {:?}", other),
        },
        other => panic!("Expected Value for duration_hours, got {:?}", other),
    }

    let duration_seconds = response.results.get("duration_seconds").unwrap();
    match &duration_seconds.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Duration(v, unit) => {
                assert_eq!(*v, Decimal::from_str("5400").unwrap());
                assert_eq!(*unit, lemma::SemanticDurationUnit::Second);
            }
            other => panic!("Expected Duration for duration_seconds, got {:?}", other),
        },
        other => panic!("Expected Value for duration_seconds, got {:?}", other),
    }

    let is_quick_processing = response.results.get("is_quick_processing").unwrap();
    assert_eq!(
        is_quick_processing.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );
}

#[test]
fn test_05_date_handling() {
    let engine = load_examples();
    let now = DateTimeValue::now();

    let mut data = std::collections::HashMap::new();
    data.insert("current_date".to_string(), "2024-06-15".to_string());

    let response = engine
        .run("date_handling", Some(&now), data, false)
        .expect("Evaluation failed");

    // Spec evaluates successfully
    assert_eq!(response.spec_name, "date_handling");

    let probation_end = response.results.get("probation_end_date").unwrap();
    match &probation_end.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Date(date) => {
                assert_eq!(date.year, 2024);
                assert_eq!(date.month, 5);
                assert_eq!(date.day, 30);
            }
            other => panic!("Expected Date for probation_end_date, got {:?}", other),
        },
        other => panic!("Expected Value for probation_end_date, got {:?}", other),
    }

    let is_probation_complete = response.results.get("is_probation_complete").unwrap();
    assert_eq!(
        is_probation_complete.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );
}

#[test]
fn test_08_rule_references() {
    let engine = load_examples();
    let now = DateTimeValue::now();

    // Test examples/rule_references spec
    let response = engine
        .run("rule_references", Some(&now), HashMap::new(), false)
        .expect("Evaluation failed");

    assert_eq!(response.spec_name, "rule_references");
    assert_eq!(
        response.results.get("can_drive_legally").unwrap().result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );

    let driving_status = response.results.get("driving_status").unwrap();
    match &driving_status.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Text(s) => assert_eq!(s, "Can drive legally"),
            other => panic!("Expected Text for driving_status, got {:?}", other),
        },
        other => panic!("Expected Value for driving_status, got {:?}", other),
    }

    // Test examples/eligibility_check spec (also in the same file)
    let response = engine
        .run("eligibility_check", Some(&now), HashMap::new(), false)
        .expect("Evaluation failed");

    assert_eq!(response.spec_name, "eligibility_check");
    assert_eq!(
        response
            .results
            .get("can_travel_internationally")
            .unwrap()
            .result,
        lemma::OperationResult::Veto(lemma::VetoType::UserDefined {
            message: Some("Valid travel documents required".to_string()),
        })
    );

    let eligibility_message = response.results.get("eligibility_message").unwrap();
    assert_eq!(
        eligibility_message.result,
        lemma::OperationResult::Veto(lemma::VetoType::UserDefined {
            message: Some("Valid travel documents required".to_string()),
        })
    );
}
