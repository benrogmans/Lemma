//! Veto functionality tests
//!
//! Key behaviors:
//! 1. Veto blocks a rule from producing any valid result
//! 2. Veto applies only when the vetoed rule's value is needed
//! 3. Unless clauses can provide alternative values, so the veto doesn't apply
//! 4. Veto in unless clause conditions or results will apply to the dependent rule

use lemma::{Engine, LiteralValue, OperationResult};
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_veto_blocks_rule_evaluation() {
    let code = r#"
spec age_check
fact age: 15
rule is_adult: age >= 18
    unless age < 18 then veto "Must be at least 18 years old"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("age_check", Some(&now), HashMap::new()).unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_adult")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Must be at least 18 years old".to_string()))
    );
}

#[test]
fn test_veto_without_message() {
    let code = r#"
spec validation
fact value: -5
rule is_valid: value > 0
    unless value < 0 then veto
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("validation", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_valid")
        .unwrap();

    assert_eq!(rule_result.result, OperationResult::Veto(None));
}

#[test]
fn test_veto_does_not_trigger_when_condition_false() {
    let code = r#"
spec age_check
fact age: 25
rule is_adult: age >= 18
    unless age < 18 then veto "Must be at least 18 years old"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("age_check", Some(&now), HashMap::new()).unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_adult")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Value(Box::new(LiteralValue::from_bool(true)))
    );
}

#[test]
fn test_multiple_veto_clauses_first_one_triggers() {
    let code = r#"
spec validation
fact age: 15
fact score: 85
rule eligible: age >= 18 and score >= 80
    unless age < 18 then veto "Age requirement not met"
    unless score < 80 then veto "Score requirement not met"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("validation", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "eligible")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Age requirement not met".to_string()))
    );
}

#[test]
fn test_multiple_veto_clauses_second_one_triggers() {
    let code = r#"
spec validation
fact age: 25
fact score: 65
rule eligible: age >= 18 and score >= 80
    unless age < 18 then veto "Age requirement not met"
    unless score < 80 then veto "Score requirement not met"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("validation", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "eligible")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Score requirement not met".to_string()))
    );
}

#[test]
fn test_veto_with_complex_condition() {
    let code = r#"
spec salary_check
fact salary: 30000
fact experience: 2
rule valid_compensation: salary >= 40000
    unless salary < 40000 and experience < 5 then veto "Insufficient salary for experience level"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("salary_check", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "valid_compensation")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Insufficient salary for experience level".to_string()))
    );
}

#[test]
fn test_veto_vs_regular_unless_mixed() {
    let code = r#"
spec mixed_validation
fact age: 20
fact country: "US"
fact has_license: false
rule can_drive: age >= 16
    unless age < 16 then veto "Too young to drive"
    unless country != "US" then false
    unless not has_license then false
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("mixed_validation", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "can_drive")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Value(Box::new(LiteralValue::from_bool(false)))
    );
}

#[test]
fn test_veto_with_number_comparison() {
    let code = r#"
spec weight_check
fact package_weight: 100
rule can_ship: package_weight <= 50
    unless package_weight > 75 then veto "Package exceeds maximum weight limit"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("weight_check", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "can_ship")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Package exceeds maximum weight limit".to_string()))
    );
}

#[test]
fn test_veto_with_money_comparison() {
    let code = r#"
spec pricing_check
fact price: 5000
rule is_affordable: price <= 1000
    unless price > 4000 then veto "Price exceeds budget limit"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("pricing_check", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_affordable")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Price exceeds budget limit".to_string()))
    );
}

#[test]
fn test_veto_with_date_comparison() {
    let code = r#"
spec date_validation
fact event_date: 2024-01-15
fact min_date: 2024-06-01
rule is_valid_date: event_date >= min_date
    unless event_date < 2024-03-01 then veto "Event date is too early in the year"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("date_validation", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_valid_date")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Event date is too early in the year".to_string()))
    );
}

#[test]
fn test_veto_with_percentage_comparison() {
    let code = r#"
spec completion_check
fact completion: 15%
rule is_complete: completion >= 95%
    unless completion < 20% then veto "Project barely started"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("completion_check", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_complete")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Project barely started".to_string()))
    );
}

#[test]
fn test_veto_with_rule_reference() {
    let code = r#"
spec eligibility
fact age: 16
fact has_permission: false
rule is_adult: age >= 18
rule eligible: has_permission
    unless not is_adult then veto "Must be adult or have permission"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("eligibility", Some(&now), HashMap::new())
        .unwrap();
    let eligible_result = response
        .results
        .values()
        .find(|r| r.rule.name == "eligible")
        .unwrap();

    assert_eq!(
        eligible_result.result,
        OperationResult::Veto(Some("Must be adult or have permission".to_string()))
    );
}

#[test]
fn test_veto_with_arithmetic_in_condition() {
    let code = r#"
spec budget_check
fact expenses: 9500
fact income: 10000
rule within_budget: expenses < income
    unless expenses > income * 0.9 then veto "Expenses exceed 90% of income"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("budget_check", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "within_budget")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Expenses exceed 90% of income".to_string()))
    );
}

#[test]
fn test_veto_with_string_equality() {
    let code = r#"
spec status_check
fact status: "cancelled"
rule is_active: status == "active"
    unless status == "cancelled" then veto "Cannot process cancelled items"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("status_check", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_active")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Cannot process cancelled items".to_string()))
    );
}

#[test]
fn test_veto_does_not_affect_other_rules() {
    let code = r#"
spec multi_rule
fact value: -10
rule check_positive: value > 0
    unless value < 0 then veto "Value must be positive"
rule check_negative: value < 0
rule double_value: value * 2
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("multi_rule", Some(&now), HashMap::new())
        .unwrap();

    let check_positive = response
        .results
        .values()
        .find(|r| r.rule.name == "check_positive")
        .unwrap();
    assert_eq!(
        check_positive.result,
        OperationResult::Veto(Some("Value must be positive".to_string()))
    );

    let check_negative = response
        .results
        .values()
        .find(|r| r.rule.name == "check_negative")
        .unwrap();
    assert_eq!(
        check_negative.result,
        OperationResult::Value(Box::new(LiteralValue::from_bool(true)))
    );

    let double_value = response
        .results
        .values()
        .find(|r| r.rule.name == "double_value")
        .unwrap();
    assert_eq!(
        double_value.result,
        OperationResult::Value(Box::new(LiteralValue::number(
            Decimal::from_str("-20.0").unwrap()
        )))
    );
}

#[test]
fn test_veto_with_empty_string_message() {
    let code = r#"
spec edge_case
fact value: 0
rule is_valid: value > 0
    unless value == 0 then veto ""
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("edge_case", Some(&now), HashMap::new()).unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_valid")
        .unwrap();

    assert!(matches!(rule_result.result, OperationResult::Veto(Some(_))));
}

#[test]
fn test_veto_with_special_characters_in_message() {
    let code = r#"
spec special_chars
fact age: 10
rule valid: age >= 18
    unless age < 18 then veto "Error: Age < 18! Must be 18+. Contact: admin@example.com (555-1234)"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("special_chars", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "valid")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some(
            "Error: Age < 18! Must be 18+. Contact: admin@example.com (555-1234)".to_string()
        ))
    );
}

#[test]
fn test_veto_with_very_long_message() {
    let message = "This is a very long veto message that contains a lot of text to test how the system handles lengthy error messages. It includes multiple sentences and should be properly stored and returned. The system should handle this without any issues regardless of the message length. Testing edge cases is important for robust software.";

    let code = format!(
        r#"
spec long_message
fact value: 0
rule valid: value > 0
    unless value == 0 then veto "{}"
"#,
        message
    );

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, &code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("long_message", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "valid")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some(message.to_string()))
    );
}

#[test]
fn test_veto_priority_over_false_result() {
    let code = r#"
spec priority_test
fact value: 5
rule check: value > 10
    unless value < 10 then veto "Value too small"
    unless value != 5 then false
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("priority_test", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "check")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Value too small".to_string()))
    );
}

#[test]
fn test_multiple_vetoes_both_conditions_true() {
    let code = r#"
spec double_veto
fact age: 15
fact score: 65
rule eligible: age >= 18 and score >= 80
    unless age < 18 then veto "Age too low"
    unless score < 80 then veto "Score too low"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("double_veto", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "eligible")
        .unwrap();

    assert!(matches!(rule_result.result, OperationResult::Veto(Some(_))));
}

#[test]
fn test_veto_with_multiple_unless_conditions() {
    let code = r#"
spec multi_unless
fact age: 30
fact has_criminal_record: true
rule eligible: age >= 18
    unless age < 18 then veto "Eligibility criteria not met"
    unless has_criminal_record then veto "Eligibility criteria not met"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("multi_unless", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "eligible")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Eligibility criteria not met".to_string()))
    );
}

#[test]
fn test_veto_with_negation() {
    let code = r#"
spec negation_test
fact is_verified: false
rule can_proceed: true
    unless not is_verified then veto "Account must be verified"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("negation_test", Some(&now), HashMap::new())
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "can_proceed")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("Account must be verified".to_string()))
    );
}
