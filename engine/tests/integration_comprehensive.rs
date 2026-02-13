use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_employee_contract_comprehensive() {
    let mut engine = Engine::new();

    let base_contract = r#"
doc base_contract
fact min_salary = 30000
fact max_salary = 200000
fact standard_vacation_days = 20 days
fact probation_period = 90 days
fact min_age = 18 years
"#;

    let employment_terms = r#"
doc employment_terms
fact base = doc base_contract
fact salary = 75000
fact bonus_percentage = 10%
fact start_date = 2024-01-15
fact vacation_days = 20 days
fact employee_age = 28 years

rule total_compensation = salary + (salary * bonus_percentage)
rule is_salary_valid = salary >= base.min_salary and salary <= base.max_salary
rule vacation_days_ok = vacation_days >= base.standard_vacation_days
rule is_adult = employee_age >= base.min_age
rule probation_end_date = start_date + base.probation_period

rule contract_valid = is_salary_valid? and vacation_days_ok? and is_adult?
    unless not is_adult? then veto "Employee must be 18 or older"
"#;

    add_lemma_code_blocking(&mut engine, base_contract, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, employment_terms, "test.lemma").unwrap();

    let response = engine
        .evaluate("employment_terms", vec![], HashMap::new())
        .unwrap();

    let total_comp = response
        .results
        .values()
        .find(|r| r.rule.name == "total_compensation")
        .unwrap();

    match &total_comp.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("82500").unwrap()),
            other => panic!("Expected Number for total_compensation, got {:?}", other),
        },
        other => panic!("Expected Value for total_compensation, got {:?}", other),
    }

    let contract_valid = response
        .results
        .values()
        .find(|r| r.rule.name == "contract_valid")
        .unwrap();
    assert_eq!(
        contract_valid.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );

    engine.remove_document("employment_terms");
    engine.remove_document("base_contract");
}

#[test]
fn test_tax_calculation_with_percentages() {
    let mut engine = Engine::new();

    let tax_doc = r#"
doc tax_calculation
fact income = 80000
fact deductions = 10000
fact tax_rate_low = 10%
fact tax_rate_mid = 20%
fact tax_rate_high = 30%
fact bracket_low = 40000
fact bracket_mid = 80000

rule taxable_income = income - deductions
rule in_low_bracket = taxable_income? <= bracket_low
rule in_mid_bracket = taxable_income? > bracket_low and taxable_income? <= bracket_mid
rule in_high_bracket = taxable_income? > bracket_mid

rule tax_rate = tax_rate_low
    unless in_mid_bracket? then tax_rate_mid
    unless in_high_bracket? then tax_rate_high

rule tax_amount = taxable_income? * tax_rate?
rule net_income = income - tax_amount?
rule effective_rate = (tax_amount? / income) * 100%
"#;

    add_lemma_code_blocking(&mut engine, tax_doc, "test.lemma").unwrap();

    let response = engine
        .evaluate("tax_calculation", vec![], HashMap::new())
        .unwrap();

    let taxable = response
        .results
        .values()
        .find(|r| r.rule.name == "taxable_income")
        .unwrap();
    match &taxable.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("70000").unwrap()),
            other => panic!("Expected Number for taxable_income, got {:?}", other),
        },
        other => panic!("Expected Value for taxable_income, got {:?}", other),
    }

    let in_mid = response
        .results
        .values()
        .find(|r| r.rule.name == "in_mid_bracket")
        .unwrap();
    assert_eq!(
        in_mid.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );

    let tax_rate = response
        .results
        .values()
        .find(|r| r.rule.name == "tax_rate")
        .unwrap();
    assert_eq!(
        tax_rate.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::ratio(
            Decimal::from_str("0.2").unwrap(),
            Some("percent".to_string())
        )))
    );

    engine.remove_document("tax_calculation");
}

#[test]
fn test_cli_fact_values_integration() {
    let mut engine = Engine::new();

    let config_doc = r#"
doc dynamic_config
fact threshold = [number]
fact multiplier = [number]
fact base_value = 100

rule calculated_value = base_value * multiplier
rule exceeds_threshold = calculated_value? > threshold
rule status = "LOW"
  unless exceeds_threshold? then "HIGH"
"#;

    add_lemma_code_blocking(&mut engine, config_doc, "test.lemma").unwrap();

    let mut facts = std::collections::HashMap::new();
    facts.insert("threshold".to_string(), "500".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let response = engine.evaluate("dynamic_config", vec![], facts).unwrap();

    let calculated = response
        .results
        .values()
        .find(|r| r.rule.name == "calculated_value")
        .unwrap();
    assert_eq!(calculated.result.value().unwrap().to_string(), "200");

    let status = response
        .results
        .values()
        .find(|r| r.rule.name == "status")
        .unwrap();
    assert_eq!(status.result.value().unwrap().to_string(), "LOW");

    let mut facts2 = std::collections::HashMap::new();
    facts2.insert("threshold".to_string(), "150".to_string());
    facts2.insert("multiplier".to_string(), "2".to_string());

    let response2 = engine.evaluate("dynamic_config", vec![], facts2).unwrap();

    let status2 = response2
        .results
        .values()
        .find(|r| r.rule.name == "status")
        .unwrap();
    assert_eq!(status2.result.value().unwrap().to_string(), "HIGH");

    engine.remove_document("dynamic_config");
}

#[test]
fn test_date_arithmetic_comprehensive() {
    let mut engine = Engine::new();

    let timeline_doc = r#"
doc project_timeline
fact project_start = 2024-01-15
fact phase1_duration = 30 days
fact phase2_duration = 45 days
fact phase3_duration = 60 days
fact today = 2024-02-15

rule phase1_end = project_start + phase1_duration
rule phase2_end = phase1_end? + phase2_duration
rule phase3_end = phase2_end? + phase3_duration

rule project_duration = phase1_duration + phase2_duration + phase3_duration
rule elapsed_time = today - project_start
rule days_remaining = phase3_end? - today

rule is_phase1_complete = today > phase1_end?
rule is_phase2_complete = today > phase2_end?
rule is_on_schedule = elapsed_time? <= phase1_duration + phase2_duration
"#;

    add_lemma_code_blocking(&mut engine, timeline_doc, "test.lemma").unwrap();

    let response = engine
        .evaluate("project_timeline", vec![], HashMap::new())
        .unwrap();

    let phase1_complete = response
        .results
        .values()
        .find(|r| r.rule.name == "is_phase1_complete")
        .unwrap();
    assert_eq!(phase1_complete.result.value().unwrap().to_string(), "true");

    let phase2_complete = response
        .results
        .values()
        .find(|r| r.rule.name == "is_phase2_complete")
        .unwrap();
    assert_eq!(
        phase2_complete.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(false)))
    );

    engine.remove_document("project_timeline");
}

// ============================================================================
// Date Arithmetic Regression Tests
// ============================================================================

#[test]
fn test_date_plus_duration() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact start = 2024-01-15
fact timespan = 30 days
rule end_date = start + timespan
"#;

    add_lemma_code_blocking(&mut engine, doc, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

    let end_date = response
        .results
        .values()
        .find(|r| r.rule.name == "end_date")
        .unwrap();

    match &end_date.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Date(date) => {
                assert_eq!(date.year, 2024);
                assert_eq!(date.month, 2);
                assert_eq!(date.day, 14);
            }
            other => panic!("Expected Date for end_date, got {:?}", other),
        },
        other => panic!("Expected Value for end_date, got {:?}", other),
    }
}

#[test]
fn test_date_minus_duration() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact end = 2024-02-14
fact timespan = 30 days
rule start_date = end - timespan
"#;

    add_lemma_code_blocking(&mut engine, doc, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

    let start_date = response
        .results
        .values()
        .find(|r| r.rule.name == "start_date")
        .unwrap();

    match &start_date.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Date(date) => {
                assert_eq!(date.year, 2024);
                assert_eq!(date.month, 1);
                assert_eq!(date.day, 15);
            }
            other => panic!("Expected Date for start_date, got {:?}", other),
        },
        other => panic!("Expected Value for start_date, got {:?}", other),
    }
}

#[test]
fn test_date_minus_date() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact start = 2024-01-15
fact end = 2024-02-14
rule timespan = end - start
"#;

    add_lemma_code_blocking(&mut engine, doc, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

    let duration = response
        .results
        .values()
        .find(|r| r.rule.name == "timespan")
        .unwrap();

    match &duration.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Duration(seconds, unit) => {
                // Date - Date returns seconds (30 days = 2,592,000 seconds)
                assert_eq!(*seconds, Decimal::from_str("2592000").unwrap());
                assert_eq!(unit.to_string(), "seconds");
            }
            other => panic!("Expected Duration for timespan, got {:?}", other),
        },
        other => panic!("Expected Value for timespan, got {:?}", other),
    }
}

#[test]
fn test_date_comparison() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact date1 = 2024-01-15
fact date2 = 2024-02-14
rule date1_before_date2 = date1 < date2
rule date1_after_date2 = date1 > date2
"#;

    add_lemma_code_blocking(&mut engine, doc, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

    let before = response
        .results
        .values()
        .find(|r| r.rule.name == "date1_before_date2")
        .unwrap();
    assert_eq!(
        before.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );

    let after = response
        .results
        .values()
        .find(|r| r.rule.name == "date1_after_date2")
        .unwrap();
    assert_eq!(
        after.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(false)))
    );
}

// ============================================================================
// Type Validation Regression Tests
// ============================================================================

#[test]
fn test_type_validation_boolean_and_number() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact flag = true
rule result_true = flag and 100 or 50
"#;

    let result = add_lemma_code_blocking(&mut engine, doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean and number in logical expression"
    );
}

#[test]
fn test_type_validation_boolean_and_money() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact needs_extra = true
rule extra_charge = needs_extra and 10 or 0
"#;

    let result = add_lemma_code_blocking(&mut engine, doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean and money in logical expression"
    );
}

#[test]
fn test_type_validation_comparison_and_number() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact value = 100
rule multiplier = value > 50 and 2 or 1
rule result = value * multiplier
"#;

    let result = add_lemma_code_blocking(&mut engine, doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean comparison result and numbers in logical expression"
    );
}

// ============================================================================
// Type Error Message Validation Tests
// ============================================================================

#[test]
fn test_logical_operator_with_text_error_message() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact system_healthy = true
rule status = system_healthy and "OK"
"#;

    let result = add_lemma_code_blocking(&mut engine, doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean and text in logical expression"
    );

    let error_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        error_msg.contains("logical")
            || error_msg.contains("boolean")
            || error_msg.contains("type"),
        "Error should mention type issue. Got: {}",
        error_msg
    );
}

#[test]
fn test_logical_or_with_text_error_message() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact flag = false
rule result = flag or "default"
"#;

    let result = add_lemma_code_blocking(&mut engine, doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean and text in 'or' expression"
    );

    let error_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        error_msg.contains("logical")
            || error_msg.contains("boolean")
            || error_msg.contains("type"),
        "Error should mention type issue. Got: {}",
        error_msg
    );
}

// ============================================================================
// Document Reference Field Access Tests
// ============================================================================

#[test]
fn test_doc_ref_field_access_simple() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact min_value = 100
fact max_value = 1000
"#;

    let child_doc = r#"
doc child
fact config = doc base
fact value = 500

rule is_valid = value >= config.min_value and value <= config.max_value
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, child_doc, "test.lemma").unwrap();

    let response = engine.evaluate("child", vec![], HashMap::new()).unwrap();

    let is_valid = response
        .results
        .values()
        .find(|r| r.rule.name == "is_valid")
        .unwrap();
    assert_eq!(
        is_valid.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );
}

#[test]
fn test_doc_ref_field_access_with_units() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact min_salary = 30000
fact max_salary = 200000
"#;

    let child_doc = r#"
doc child
fact base_contract = doc base
fact salary = 75000

rule is_valid = salary >= base_contract.min_salary and salary <= base_contract.max_salary
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, child_doc, "test.lemma").unwrap();

    let response = engine.evaluate("child", vec![], HashMap::new()).unwrap();

    let is_valid = response
        .results
        .values()
        .find(|r| r.rule.name == "is_valid")
        .unwrap();
    assert_eq!(
        is_valid.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );
}

#[test]
fn test_doc_ref_field_access_arithmetic() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact project_start = 2024-01-15
fact probation_period = 90 days
"#;

    let child_doc = r#"
doc child
fact base_contract = doc base

rule probation_end = base_contract.project_start + base_contract.probation_period
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, child_doc, "test.lemma").unwrap();

    let response = engine.evaluate("child", vec![], HashMap::new()).unwrap();

    let probation_end = response
        .results
        .values()
        .find(|r| r.rule.name == "probation_end")
        .unwrap();

    match &probation_end.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Date(date) => {
                assert_eq!(date.year, 2024);
                assert_eq!(date.month, 4);
                assert_eq!(date.day, 14);
            }
            other => panic!("Expected Date for probation_end, got {:?}", other),
        },
        other => panic!("Expected Value for probation_end, got {:?}", other),
    }
}
