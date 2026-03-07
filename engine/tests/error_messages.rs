mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, Error};
use std::collections::HashMap;

/// Test suite for error messages as documented in ERROR_MESSAGES_IMPLEMENTATION.md
/// Covers parse errors, semantic errors, and runtime errors with proper span tracking

// ============================================================================
// VALIDATION ERRORS - Duplicate Definitions
// ============================================================================

#[test]
fn test_duplicate_fact_definition_error() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact salary: 50000
        fact salary: 60000
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    let msg = &details.message;
    assert!(
        msg.to_lowercase().contains("duplicate") && msg.to_lowercase().contains("fact"),
        "Error should mention duplicate fact, got: {msg}"
    );
    assert!(
        msg.contains("salary"),
        "Error should mention fact name, got: {msg}"
    );
}

#[test]
fn test_duplicate_rule_definition_error() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact x: 10
        rule total: x * 2
        rule total: x * 3
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    let msg = &details.message;
    assert!(
        msg.to_lowercase().contains("duplicate") && msg.to_lowercase().contains("rule"),
        "Error should mention duplicate rule, got: {msg}"
    );
    assert!(
        msg.contains("total"),
        "Error should mention rule name, got: {msg}"
    );
}

#[test]
fn test_duplicate_fact_shows_name() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact name: "Alice"
        fact age: 30
        fact name: "Bob"
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    let msg = &details.message;
    assert!(
        msg.contains("Duplicate"),
        "Error should mention duplicate, got: {msg}"
    );
    assert!(
        msg.contains("name"),
        "Error should mention fact name, got: {msg}"
    );
}

// ============================================================================
// RUNTIME ERRORS - Division by Zero (now returns Veto, not Error)
// ============================================================================

#[test]
fn test_runtime_error_division_by_zero() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact numerator: 100
        fact denominator: 0
        rule result: numerator / denominator
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test", None, &now, vec![], HashMap::new())
        .expect("Division by zero should return Veto, not Error");

    let result_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");

    assert!(
        result_rule.result.vetoed(),
        "Division by zero should return Veto, got: {:?}",
        result_rule.result
    );

    if let lemma::OperationResult::Veto(Some(msg)) = &result_rule.result {
        assert!(
            msg.to_lowercase().contains("division") || msg.to_lowercase().contains("zero"),
            "Veto message should mention division or zero, got: {}",
            msg
        );
    }
}

#[test]
fn test_runtime_error_division_by_zero_with_cli_facts() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact hours_worked: [number]
        fact salary: 50000
        rule hourly_rate: salary / hours_worked
    "#,
        "test.lemma",
    )
    .unwrap();

    let mut facts = std::collections::HashMap::new();
    facts.insert("hours_worked".to_string(), "0".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test", None, &now, vec![], facts)
        .expect("Division by zero should return Veto, not Error");

    let hourly_rate = response
        .results
        .values()
        .find(|r| r.rule.name == "hourly_rate")
        .expect("hourly_rate rule should exist");

    assert!(
        hourly_rate.result.vetoed(),
        "Division by zero should return Veto, got: {:?}",
        hourly_rate.result
    );
}

// ============================================================================
// RUNTIME ERRORS - Circular Dependencies
// ============================================================================
// Note: Cross-rule circular dependencies currently cause Prolog to hang
// Testing is limited to self-referencing rules which are caught at transpilation time

#[test]
fn test_transpile_error_self_referencing_rule() {
    let mut engine = Engine::new();

    // Self-referencing rules are caught during transpilation
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        rule x: x + 1
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    let msg = &details.message;
    assert!(msg.to_lowercase().contains("circular") || msg.to_lowercase().contains("itself"));
    assert!(msg.contains("x"));
}

// ============================================================================
// VALIDATION ERRORS - Type Mismatches (now caught at validation time)
// ============================================================================

#[test]
fn test_validation_error_type_mismatch_text_in_arithmetic() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact name: "Alice"
        fact salary: 50000
        rule result: salary + name
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Cannot apply"));
}

#[test]
fn test_validation_error_boolean_in_arithmetic() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact is_active: true
        fact count: 10
        rule result: count * is_active
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Cannot apply"));
}

// ============================================================================
// ERROR MESSAGE FORMATTING - Duplicate Detection
// ============================================================================

#[test]
fn test_duplicate_error_contains_fact_name() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc my_document
        fact price: 100
        fact price: 200
    "#,
        "my_file.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Duplicate"));
    assert!(details.message.contains("price"));
}

#[test]
fn test_duplicate_error_is_reported() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact x: 10
        fact x: 20
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Duplicate"));
    assert!(details.message.contains("x"));
}

#[test]
fn test_duplicate_in_second_doc_is_caught() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc first_doc
        fact a: 1

        doc second_doc
        fact b: 2
        fact b: 3
    "#,
        "multi.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Duplicate"));
    assert!(details.message.contains("b"));
}

// ============================================================================
// ERROR MESSAGE DISPLAY
// ============================================================================

#[test]
fn test_error_display_contains_duplicate_info() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact value: 100
        fact value: 200
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Duplicate"));
    assert!(details.message.contains("value"));
}

// ============================================================================
// VETO MESSAGES - Division by Zero
// ============================================================================

#[test]
fn test_division_by_zero_returns_veto_with_message() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact x: 100
        fact y: 0
        rule result: x / y
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test", None, &now, vec![], HashMap::new())
        .expect("Should return Veto, not Error");

    let result_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");

    match &result_rule.result {
        lemma::OperationResult::Veto(Some(msg)) => {
            assert!(
                msg.to_lowercase().contains("zero") || msg.to_lowercase().contains("division"),
                "Veto message should mention zero or division, got: {}",
                msg
            );
        }
        lemma::OperationResult::Veto(None) => {
            panic!("Expected Veto with message");
        }
        other => panic!("Expected Veto, got: {:?}", other),
    }
}

#[test]
fn test_circular_dependency_has_helpful_suggestion() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        rule x: y
        rule y: x
    "#,
        "test.lemma",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    let msg = &details.message;
    assert!(msg.to_lowercase().contains("circular") || msg.to_lowercase().contains("cycle"));
    assert!(msg.contains("x") && msg.contains("y"));
}

// ============================================================================
// DUPLICATE DETECTION ACCURACY
// ============================================================================

#[test]
fn test_duplicate_fact_is_detected() {
    let mut engine = Engine::new();

    let lemma_code = r#"doc test
fact line2: 1
fact line3: 2
fact line4: 3
fact line4: 4"#;

    let result = add_lemma_code_blocking(&mut engine, lemma_code, "test.lemma");

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Duplicate"));
    assert!(details.message.contains("line4"));
}

#[test]
fn test_division_by_zero_returns_veto() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        doc test
        fact numerator: 42
        fact denominator: 0
        rule division_result: numerator / denominator
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test", None, &now, vec![], HashMap::new())
        .expect("Should return Veto, not Error");

    let division_result = response
        .results
        .values()
        .find(|r| r.rule.name == "division_result")
        .expect("division_result rule should exist");

    assert!(
        division_result.result.vetoed(),
        "Division by zero should return Veto, got: {:?}",
        division_result.result
    );
}

// ============================================================================
// DUPLICATE DETECTION FROM VARIOUS SOURCES
// ============================================================================

#[test]
fn test_duplicate_detected_from_database_source() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc contract
        fact amount: 1000
        fact amount: 2000
    "#,
        "db://contracts/123",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Duplicate"));
    assert!(details.message.contains("amount"));
}

#[test]
fn test_duplicate_detected_from_api_source() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc policy
        rule rate: 1.5
        rule rate: 2.0
    "#,
        "api://policies/endpoint",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Duplicate"));
    assert!(details.message.contains("rate"));
}

#[test]
fn test_duplicate_detected_from_runtime_source() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc runtime_doc
        fact x: 5
        fact x: 10
    "#,
        "<runtime>",
    );

    let errs = result.unwrap_err();
    let details = errs
        .iter()
        .find_map(|e| match e {
            Error::Validation(d) => Some(d),
            _ => None,
        })
        .expect("expected at least one Validation error");
    assert!(details.message.contains("Duplicate"));
    assert!(details.message.contains("x"));
}

// ============================================================================
// MULTI-ERROR COLLECTION - Graph building errors + type checking errors
// ============================================================================

/// Regression test: the engine must report errors from BOTH graph building
/// (e.g. missing reference) and type checking (e.g. branch type
/// mismatch) in a single pass.  Previously, graph building errors caused an
/// early return that prevented type checking from running at all.
#[test]
fn test_multiple_error_phases_reported_together() {
    let mut engine = Engine::new();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
        doc pricing

        type money: scale
          -> unit eur 1
          -> unit usd 1.19

        fact price    : [money]
        fact quantity : [number -> minimum 0]
        fact is_member: false

        rule discount: 0%
          unless quantity >= 10 then 10%
          unless quantity >= 50 then 20%
          unless is_member then 15

        rule total: price * quantity - non_existent_rule
          unless price > 100 usd then veto "This price is too high."
    "#,
        "pricing.lemma",
    );

    let errs = result.unwrap_err();
    let messages: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
    let has_rule_ref_error = messages
        .iter()
        .any(|m| m.contains("non_existent_rule") && m.contains("not found"));
    let has_type_mismatch = messages
        .iter()
        .any(|m| m.contains("Type mismatch") || m.contains("type mismatch"));
    assert!(
        has_rule_ref_error,
        "Should report missing reference. Got: {messages:?}"
    );
    assert!(
        has_type_mismatch,
        "Should report type mismatch (15 is number, not ratio). Got: {messages:?}"
    );
}
