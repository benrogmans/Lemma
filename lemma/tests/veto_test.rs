//! Veto functionality tests
//!
//! Key behaviors:
//! 1. Veto blocks a rule from producing any valid result
//! 2. Veto applies only when the vetoed rule's value is needed
//! 3. Unless clauses can provide alternative values, so the veto doesn't apply
//! 4. Veto in unless clause conditions or results will apply to the dependent rule

use lemma::{Engine, LiteralValue};
use rust_decimal::Decimal;
use std::str::FromStr;

#[test]
fn test_veto_blocks_rule_evaluation() {
    let code = r#"
doc age_check
fact age = 15
rule is_adult = age >= 18
    unless age < 18 then veto "Must be at least 18 years old"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("age_check", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_adult")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Must be at least 18 years old".to_string())
    );
}

#[test]
fn test_veto_without_message() {
    let code = r#"
doc validation
fact value = -5
rule is_valid = value > 0
    unless value < 0 then veto
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("validation", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_valid")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(rule_result.veto_message, None);
}

#[test]
fn test_veto_does_not_trigger_when_condition_false() {
    let code = r#"
doc age_check
fact age = 25
rule is_adult = age >= 18
    unless age < 18 then veto "Must be at least 18 years old"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("age_check", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_adult")
        .unwrap();

    assert_eq!(rule_result.result, Some(LiteralValue::Boolean(true)));
    assert_eq!(rule_result.veto_message, None);
}

#[test]
fn test_multiple_veto_clauses_first_one_triggers() {
    let code = r#"
doc validation
fact age = 15
fact score = 85
rule eligible = age >= 18 and score >= 80
    unless age < 18 then veto "Age requirement not met"
    unless score < 80 then veto "Score requirement not met"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("validation", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "eligible")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Age requirement not met".to_string())
    );
}

#[test]
fn test_multiple_veto_clauses_second_one_triggers() {
    let code = r#"
doc validation
fact age = 25
fact score = 65
rule eligible = age >= 18 and score >= 80
    unless age < 18 then veto "Age requirement not met"
    unless score < 80 then veto "Score requirement not met"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("validation", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "eligible")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Score requirement not met".to_string())
    );
}

#[test]
fn test_veto_with_complex_condition() {
    let code = r#"
doc salary_check
fact salary = 30000
fact experience = 2
rule valid_compensation = salary >= 40000
    unless salary < 40000 and experience < 5 then veto "Insufficient salary for experience level"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("salary_check", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "valid_compensation")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Insufficient salary for experience level".to_string())
    );
}

#[test]
fn test_veto_vs_regular_unless_mixed() {
    let code = r#"
doc mixed_validation
fact age = 20
fact country = "US"
fact has_license = false
rule can_drive = age >= 16
    unless age < 16 then veto "Too young to drive"
    unless country != "US" then false
    unless not has_license then false
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("mixed_validation", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "can_drive")
        .unwrap();

    assert_eq!(rule_result.result, Some(LiteralValue::Boolean(false)));
    assert_eq!(rule_result.veto_message, None);
}

#[test]
fn test_veto_with_unit_comparison() {
    let code = r#"
doc weight_check
fact package_weight = 100 kilograms
rule can_ship = package_weight <= 50 kilograms
    unless package_weight > 75 kilograms then veto "Package exceeds maximum weight limit"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("weight_check", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "can_ship")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Package exceeds maximum weight limit".to_string())
    );
}

#[test]
fn test_veto_with_money_comparison() {
    let code = r#"
doc pricing_check
fact price = 5000
rule is_affordable = price <= 1000
    unless price > 4000 then veto "Price exceeds budget limit"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("pricing_check", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_affordable")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Price exceeds budget limit".to_string())
    );
}

#[test]
fn test_veto_with_date_comparison() {
    let code = r#"
doc date_validation
fact event_date = 2024-01-15
fact min_date = 2024-06-01
rule is_valid_date = event_date >= min_date
    unless event_date < 2024-03-01 then veto "Event date is too early in the year"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("date_validation", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_valid_date")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Event date is too early in the year".to_string())
    );
}

#[test]
fn test_veto_with_percentage_comparison() {
    let code = r#"
doc completion_check
fact completion = 15%
rule is_complete = completion >= 95%
    unless completion < 20% then veto "Project barely started"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("completion_check", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_complete")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Project barely started".to_string())
    );
}

#[test]
fn test_veto_with_rule_reference() {
    let code = r#"
doc eligibility
fact age = 16
fact has_permission = false
rule is_adult = age >= 18
rule eligible = has_permission
    unless not is_adult? then veto "Must be adult or have permission"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("eligibility", None, None).unwrap();
    let eligible_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "eligible")
        .unwrap();

    assert_eq!(eligible_result.result, None);
    assert_eq!(
        eligible_result.veto_message,
        Some("Must be adult or have permission".to_string())
    );
}

#[test]
fn test_veto_with_arithmetic_in_condition() {
    let code = r#"
doc budget_check
fact expenses = 9500
fact income = 10000
rule within_budget = expenses < income
    unless expenses > income * 0.9 then veto "Expenses exceed 90% of income"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("budget_check", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "within_budget")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Expenses exceed 90% of income".to_string())
    );
}

#[test]
fn test_veto_with_string_equality() {
    let code = r#"
doc status_check
fact status = "cancelled"
rule is_active = status == "active"
    unless status == "cancelled" then veto "Cannot process cancelled items"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("status_check", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_active")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Cannot process cancelled items".to_string())
    );
}

#[test]
fn test_veto_does_not_affect_other_rules() {
    let code = r#"
doc multi_rule
fact value = -10
rule check_positive = value > 0
    unless value < 0 then veto "Value must be positive"
rule check_negative = value < 0
rule double_value = value * 2
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("multi_rule", None, None).unwrap();

    let check_positive = response
        .results
        .iter()
        .find(|r| r.rule_name == "check_positive")
        .unwrap();
    assert_eq!(check_positive.result, None);
    assert_eq!(
        check_positive.veto_message,
        Some("Value must be positive".to_string())
    );

    let check_negative = response
        .results
        .iter()
        .find(|r| r.rule_name == "check_negative")
        .unwrap();
    assert_eq!(check_negative.result, Some(LiteralValue::Boolean(true)));
    assert_eq!(check_negative.veto_message, None);

    let double_value = response
        .results
        .iter()
        .find(|r| r.rule_name == "double_value")
        .unwrap();
    assert_eq!(
        double_value.result,
        Some(LiteralValue::Number(Decimal::from_str("-20.0").unwrap()))
    );
    assert_eq!(double_value.veto_message, None);
}

#[test]
fn test_veto_with_empty_string_message() {
    let code = r#"
doc edge_case
fact value = 0
rule is_valid = value > 0
    unless value == 0 then veto ""
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("edge_case", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_valid")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert!(rule_result.veto_message.is_some());
}

#[test]
fn test_veto_with_special_characters_in_message() {
    let code = r#"
doc special_chars
fact age = 10
rule valid = age >= 18
    unless age < 18 then veto "Error: Age < 18! Must be 18+. Contact: admin@example.com (555-1234)"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("special_chars", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "valid")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Error: Age < 18! Must be 18+. Contact: admin@example.com (555-1234)".to_string())
    );
}

#[test]
fn test_veto_with_very_long_message() {
    let message = "This is a very long veto message that contains a lot of text to test how the system handles lengthy error messages. It includes multiple sentences and should be properly stored and returned. The system should handle this without any issues regardless of the message length. Testing edge cases is important for robust software.";

    let code = format!(
        r#"
doc long_message
fact value = 0
rule valid = value > 0
    unless value == 0 then veto "{}"
"#,
        message
    );

    let mut engine = Engine::new();
    engine.add_lemma_code(&code, "test.lemma").unwrap();

    let response = engine.evaluate("long_message", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "valid")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(rule_result.veto_message, Some(message.to_string()));
}

#[test]
fn test_veto_priority_over_false_result() {
    let code = r#"
doc priority_test
fact value = 5
rule check = value > 10
    unless value < 10 then veto "Value too small"
    unless value != 5 then false
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("priority_test", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "check")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Value too small".to_string())
    );
}

#[test]
fn test_multiple_vetoes_both_conditions_true() {
    let code = r#"
doc double_veto
fact age = 15
fact score = 65
rule eligible = age >= 18 and score >= 80
    unless age < 18 then veto "Age too low"
    unless score < 80 then veto "Score too low"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("double_veto", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "eligible")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert!(rule_result.veto_message.is_some());
}

#[test]
fn test_veto_with_or_condition() {
    let code = r#"
doc or_condition
fact age = 30
fact has_criminal_record = true
rule eligible = age >= 18
    unless age < 18 or has_criminal_record then veto "Eligibility criteria not met"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("or_condition", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "eligible")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Eligibility criteria not met".to_string())
    );
}

#[test]
fn test_veto_with_negation() {
    let code = r#"
doc negation_test
fact is_verified = false
rule can_proceed = true
    unless not is_verified then veto "Account must be verified"
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("negation_test", None, None).unwrap();
    let rule_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "can_proceed")
        .unwrap();

    assert_eq!(rule_result.result, None);
    assert_eq!(
        rule_result.veto_message,
        Some("Account must be verified".to_string())
    );
}
