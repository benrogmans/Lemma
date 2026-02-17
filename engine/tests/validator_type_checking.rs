use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;

#[test]
fn test_logical_and_requires_boolean_operands() {
    let code = r#"
doc test
rule result = 5 and true
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(result.is_err(), "Should reject non-boolean in 'and'");
    assert!(result.unwrap_err().to_string().contains("boolean"));
}

#[test]
fn test_logical_or_requires_boolean_operands() {
    let code = r#"
doc test
rule result = "hello" or false
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(result.is_err(), "Should reject non-boolean in 'or'");
    assert!(result.unwrap_err().to_string().contains("boolean"));
}

#[test]
fn test_unless_condition_must_be_boolean() {
    let code = r#"
doc test
rule result = 10
  unless 5 then 20
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(result.is_err(), "Unless condition must be boolean");
}

#[test]
fn test_percentage_literal_type() {
    let code = r#"
doc test
fact rate = 15%
rule doubled = rate
  unless rate > 10% then 20%
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Percentage types should be consistent: {:?}",
        result
    );
}

#[test]
fn test_text_number_comparison_allowed() {
    let code = r#"
doc test
fact name = "Alice"
fact age = 30
rule check = name == "Bob" and age > 25
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Text and number comparisons should be allowed separately: {:?}",
        result
    );
}

#[test]
fn test_date_comparison() {
    let code = r#"
doc test
fact start = 2024-01-01
fact end = 2024-12-31
rule is_valid_range = end > start
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Date comparison should be allowed: {:?}",
        result
    );
}

#[test]
fn test_duration_conversion() {
    // Duration is the only remaining built-in unit type
    let code = r#"
doc test
fact value = 60
rule converted = (value * 60) in seconds
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Duration conversion should work: {:?}",
        result
    );
}

#[test]
fn test_percentage_conversion_from_number() {
    let code = r#"
doc test
fact ratio = 0.25
rule as_percentage = ratio in percent
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Number to percentage conversion should work: {:?}",
        result
    );
}

#[test]
fn test_veto_type_is_compatible_with_other_types() {
    let code = r#"
doc test
fact age = 15
rule result = 100
  unless age < 18 then veto "Too young"
  unless age > 65 then 50
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Veto should not conflict with other return types: {:?}",
        result
    );
}

#[test]
fn test_mixed_text_and_number_not_allowed() {
    let code = r#"
doc test
fact flag = true
rule value = "default"
  unless flag then 42
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing text and number types"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
        "Error message should contain type mismatch info: {}",
        err_msg
    );
}

#[test]
fn test_mixed_date_and_number_not_allowed() {
    let code = r#"
doc test
fact use_date = true
rule value = 2024-01-01
  unless use_date then 100
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing date and number types"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
        "Error message should contain type mismatch info: {}",
        err_msg
    );
}

#[test]
fn test_boolean_consistency() {
    let code = r#"
doc test
fact x = 5
fact y = 10
rule check = x < y
  unless x == 0 then y > 0
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Boolean results should be consistent: {:?}",
        result
    );
}

#[test]
fn test_arithmetic_result_type_inference() {
    let code = r#"
doc test
fact a = 10
fact b = 20
rule sum = a + b
  unless a == 0 then 0
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Arithmetic should infer number type: {:?}",
        result
    );
}

#[test]
fn test_multiple_unless_clauses_type_consistency() {
    let code = r#"
doc test
fact x = 5
rule value = 10
  unless x < 0 then 0
  unless x > 100 then 100
  unless x == 5 then 5
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "All number branches should be consistent: {:?}",
        result
    );
}

#[test]
fn test_multiple_unless_clauses_type_inconsistency() {
    let code = r#"
doc test
fact x = 5
rule value = 10
  unless x < 0 then 0
  unless x > 100 then "overflow"
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(result.is_err(), "Mixed number/text should be rejected");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
        "Error message should contain type mismatch info: {}",
        err_msg
    );
}

#[test]
fn test_rule_reference_type_propagation() {
    let code = r#"
doc test
fact base = 100
rule derived = base * 2
rule another = derived?
  unless derived? > 150 then 0
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Rule reference types should propagate: {:?}",
        result
    );
}

#[test]
fn test_time_type_validation() {
    let code = r#"
doc test
fact meeting_time = 14:30:00
rule is_afternoon = meeting_time > 12:00:00
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Time type should be validated correctly: {:?}",
        result
    );
}

#[test]
fn test_time_cannot_use_in_logical_operators() {
    let code = r#"
doc test
fact time1 = 14:30:00
fact time2 = 15:00:00
rule result = time1 and time2
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject time values in logical operators"
    );
    assert!(result.unwrap_err().to_string().contains("boolean"));
}

#[test]
fn test_mixed_time_and_number_not_allowed() {
    let code = r#"
doc test
fact use_time = true
rule value = 14:30:00
  unless use_time then 100
"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing time and number types"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
        "Error message should contain type mismatch info: {}",
        err_msg
    );
}
