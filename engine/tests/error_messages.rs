use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, Error};
use std::collections::HashMap;

/// Test suite for error messages as documented in ERROR_MESSAGES_IMPLEMENTATION.md
/// Covers parse errors, semantic errors, and runtime errors with proper span tracking

// ============================================================================
// VALIDATION ERRORS - Duplicate Definitions
// ============================================================================

#[test]
fn test_duplicate_data_definition_error() {
    let mut engine = Engine::new();

    let result = engine.load(
        r#"
        spec test
        data salary: 50000
        data salary: 60000
    "#,
        lemma::SourceType::Labeled("test.lemma"),
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
        msg.to_lowercase().contains("duplicate") && msg.to_lowercase().contains("data"),
        "Error should mention duplicate data, got: {msg}"
    );
    assert!(
        msg.contains("salary"),
        "Error should mention data name, got: {msg}"
    );
}

#[test]
fn test_duplicate_rule_definition_error() {
    let mut engine = Engine::new();

    let result = engine.load(
        r#"
        spec test
        data x: 10
        rule total: x * 2
        rule total: x * 3
    "#,
        lemma::SourceType::Labeled("test.lemma"),
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
fn test_duplicate_data_shows_name() {
    let mut engine = Engine::new();

    let result = engine.load(
        r#"
        spec test
        data name: "Alice"
        data age: 30
        data name: "Bob"
    "#,
        lemma::SourceType::Labeled("test.lemma"),
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
        "Error should mention data name, got: {msg}"
    );
}

// ============================================================================
// RUNTIME ERRORS - Division by Zero (now returns Veto, not Error)
// ============================================================================

#[test]
fn test_runtime_error_division_by_zero() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec test
        data numerator: 100
        data denominator: 0
        rule result: numerator / denominator
    "#,
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), HashMap::new(), false)
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

    if let lemma::OperationResult::Veto(lemma::VetoType::Computation { message }) =
        &result_rule.result
    {
        assert!(
            message.to_lowercase().contains("division") || message.to_lowercase().contains("zero"),
            "Veto message should mention division or zero, got: {}",
            message
        );
    }
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
    let result = engine.load(
        r#"
        spec test
        rule x: x + 1
    "#,
        lemma::SourceType::Labeled("test.lemma"),
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
// ERROR MESSAGE FORMATTING - Duplicate Detection
// ============================================================================

#[test]
fn test_duplicate_error_contains_data_name() {
    let mut engine = Engine::new();

    let result = engine.load(
        r#"
        spec my_spec
        data price: 100
        data price: 200
    "#,
        lemma::SourceType::Labeled("my_file.lemma"),
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

    let result = engine.load(
        r#"
        spec test
        data x: 10
        data x: 20
    "#,
        lemma::SourceType::Labeled("test.lemma"),
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
fn test_duplicate_in_second_spec_is_caught() {
    let mut engine = Engine::new();

    let result = engine.load(
        r#"
        spec first_spec
        data a: 1

        spec second_spec
        data b: 2
        data b: 3
    "#,
        lemma::SourceType::Labeled("multi.lemma"),
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

    let result = engine.load(
        r#"
        spec test
        data value: 100
        data value: 200
    "#,
        lemma::SourceType::Labeled("test.lemma"),
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

    engine
        .load(
            r#"
        spec test
        data x: 100
        data y: 0
        rule result: x / y
    "#,
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .expect("Should return Veto, not Error");

    let result_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");

    match &result_rule.result {
        lemma::OperationResult::Veto(lemma::VetoType::Computation { message }) => {
            assert!(
                message.to_lowercase().contains("zero")
                    || message.to_lowercase().contains("division"),
                "Veto message should mention zero or division, got: {}",
                message
            );
        }
        other => panic!("Expected Veto, got: {:?}", other),
    }
}

#[test]
fn test_circular_dependency_has_helpful_suggestion() {
    let mut engine = Engine::new();

    let result = engine.load(
        r#"
        spec test
        rule x: y
        rule y: x
    "#,
        lemma::SourceType::Labeled("test.lemma"),
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
fn test_duplicate_data_is_detected() {
    let mut engine = Engine::new();

    let lemma_code = r#"spec test
data line2: 1
data line3: 2
data line4: 3
data line4: 4"#;

    let result = engine.load(lemma_code, lemma::SourceType::Labeled("test.lemma"));

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

// ============================================================================
// DUPLICATE DETECTION FROM VARIOUS SOURCES
// ============================================================================

#[test]
fn test_duplicate_detected_from_database_source() {
    let mut engine = Engine::new();

    let result = engine.load(
        r#"
        spec contract
        data amount: 1000
        data amount: 2000
    "#,
        lemma::SourceType::Labeled("db://contracts/123"),
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

    let result = engine.load(
        r#"
        spec pricing

        data money: scale
          -> unit eur 1
          -> unit usd 1.19

        data price    : money
        data quantity : number -> minimum 0
        data is_member: false

        rule discount: 0%
          unless quantity >= 10 then 10%
          unless quantity >= 50 then 20%
          unless is_member then 15

        rule total: price * quantity - non_existent_rule
          unless price > 100 usd then veto "This price is too high."
    "#,
        lemma::SourceType::Labeled("pricing.lemma"),
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

// ============================================================================
// TEMPORAL / EffectiveDate::Origin — error text must stay readable
// ============================================================================

#[test]
fn unversioned_spec_missing_dep_message_names_specs() {
    let mut engine = Engine::new();
    let result = engine.load(
        r#"
spec app
with z: no_such_dep
rule r: 1
"#,
        lemma::SourceType::Labeled("t.lemma"),
    );
    let errs = result.expect_err("missing dep");
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.contains("no_such_dep") && joined.contains("app"),
        "expected naming both specs: {joined}"
    );
    assert!(
        !joined.ends_with("active at "),
        "message must not truncate after 'active at' with empty instant: {joined}"
    );
}

#[test]
fn unversioned_consumer_temporal_coverage_gap_names_consumer_and_dep() {
    let mut engine = Engine::new();
    let result = engine.load(
        r#"
spec app
with d: dep
rule r: d.x

spec dep 2025-12-01
data x: 1
"#,
        lemma::SourceType::Labeled("gap.lemma"),
    );
    let errs = result.expect_err("coverage gap");
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.contains("app") && joined.contains("dep"),
        "expected both spec names in coverage error: {joined}"
    );
}
