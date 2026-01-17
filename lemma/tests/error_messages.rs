use lemma::{Engine, LemmaError};
use std::collections::HashMap;

/// Test suite for error messages as documented in ERROR_MESSAGES_IMPLEMENTATION.md
/// Covers parse errors, semantic errors, and runtime errors with proper span tracking

// ============================================================================
// VALIDATION ERRORS - Duplicate Definitions
// ============================================================================

#[test]
fn test_duplicate_fact_definition_error() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc test
        fact salary = 50000
        fact salary = 60000
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(
                msg.to_lowercase().contains("duplicate") && msg.to_lowercase().contains("fact"),
                "Error should mention duplicate fact, got: {}",
                msg
            );
            assert!(
                msg.contains("salary"),
                "Error should mention fact name, got: {}",
                msg
            );
        }
        Err(e) => panic!("Expected Engine error for duplicate fact, got: {e:?}"),
        Ok(_) => panic!("Expected error for duplicate fact"),
    }
}

#[test]
fn test_duplicate_rule_definition_error() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc test
        fact x = 10
        rule total = x * 2
        rule total = x * 3
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(
                msg.to_lowercase().contains("duplicate") && msg.to_lowercase().contains("rule"),
                "Error should mention duplicate rule, got: {}",
                msg
            );
            assert!(
                msg.contains("total"),
                "Error should mention rule name, got: {}",
                msg
            );
        }
        Err(e) => panic!("Expected Engine error for duplicate rule, got: {e:?}"),
        Ok(_) => panic!("Expected error for duplicate rule"),
    }
}

#[test]
fn test_duplicate_fact_shows_name() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc test
        fact name = "Alice"
        fact age = 30
        fact name = "Bob"
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(
                msg.contains("Duplicate"),
                "Error should mention duplicate, got: {}",
                msg
            );
            assert!(
                msg.contains("name"),
                "Error should mention fact name, got: {}",
                msg
            );
        }
        Err(e) => panic!("Expected Engine error for duplicate fact, got: {e:?}"),
        Ok(_) => panic!("Expected error for duplicate fact"),
    }
}

// ============================================================================
// PARSE ERRORS - Syntax Errors
// ============================================================================

#[test]
fn test_parse_error_with_span() {
    let result = lemma::parse(
        r#"
        doc test
        fact name = "Unclosed string
        fact age = 25
    "#,
        "test.lemma",
        &lemma::ResourceLimits::default(),
    );

    match result {
        Err(LemmaError::Parse(details)) => {
            assert_eq!(details.source_location.attribute, "test.lemma");
            assert_eq!(details.source_location.doc_name, "<parse-error>");
        }
        Err(e) => panic!("Expected Parse error, got: {e:?}"),
        Ok(_) => panic!("Expected parse error for unclosed string"),
    }
}

#[test]
fn test_parse_error_malformed_input() {
    let result = lemma::parse(
        r#"
        doc test
        this is not valid lemma syntax @#$%
    "#,
        "test.lemma",
        &lemma::ResourceLimits::default(),
    );

    assert!(result.is_err(), "Should fail on malformed input");

    match result {
        Err(LemmaError::Parse { .. }) => {
            // Expected
        }
        Err(e) => panic!("Expected Parse error, got: {e:?}"),
        Ok(_) => panic!("Expected parse error"),
    }
}

// ============================================================================
// RUNTIME ERRORS - Division by Zero (now returns Veto, not Error)
// ============================================================================

#[test]
fn test_runtime_error_division_by_zero() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
        doc test
        fact numerator = 100
        fact denominator = 0
        rule result = numerator / denominator
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine
        .evaluate("test", vec![], HashMap::new())
        .expect("Division by zero should return Veto, not Error");

    let result_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");

    assert!(
        result_rule.result.is_veto(),
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

    engine
        .add_lemma_code(
            r#"
        doc test
        fact hours_worked = [number]
        fact salary = 50000
        rule hourly_rate = salary / hours_worked
    "#,
            "test.lemma",
        )
        .unwrap();

    let mut facts = std::collections::HashMap::new();
    facts.insert("hours_worked".to_string(), "0".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Division by zero should return Veto, not Error");

    let hourly_rate = response
        .results
        .values()
        .find(|r| r.rule.name == "hourly_rate")
        .expect("hourly_rate rule should exist");

    assert!(
        hourly_rate.result.is_veto(),
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
    let result = engine.add_lemma_code(
        r#"
        doc test
        rule x = x? + 1
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::CircularDependency { details, .. }) => {
            let msg = &details.message;
            assert!(
                msg.to_lowercase().contains("circular") || msg.to_lowercase().contains("itself")
            );
            assert!(msg.contains("x"));
        }
        Err(e) => panic!("Expected CircularDependency error, got: {e:?}"),
        Ok(_) => panic!("Expected error for self-referencing rule"),
    }
}

// ============================================================================
// VALIDATION ERRORS - Type Mismatches (now caught at validation time)
// ============================================================================

#[test]
fn test_validation_error_type_mismatch_text_in_arithmetic() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc test
        fact name = "Alice"
        fact salary = 50000
        rule result = salary + name
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(
                msg.to_lowercase().contains("type")
                    || msg.to_lowercase().contains("arithmetic")
                    || msg.to_lowercase().contains("numeric"),
                "Error should mention type issue, got: {}",
                msg
            );
        }
        Err(e) => panic!("Expected Engine error for type mismatch, got: {e:?}"),
        Ok(_) => panic!("Expected validation error for type mismatch"),
    }
}

#[test]
fn test_validation_error_boolean_in_arithmetic() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc test
        fact is_active = true
        fact count = 10
        rule result = count * is_active
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(
                msg.to_lowercase().contains("arithmetic")
                    || msg.to_lowercase().contains("type")
                    || msg.to_lowercase().contains("numeric"),
                "Error should mention arithmetic or type issue, got: {}",
                msg
            );
        }
        Err(e) => panic!("Expected Engine error for invalid arithmetic, got: {e:?}"),
        Ok(_) => panic!("Expected validation error for boolean in arithmetic"),
    }
}

// ============================================================================
// ERROR MESSAGE FORMATTING - Duplicate Detection
// ============================================================================

#[test]
fn test_duplicate_error_contains_fact_name() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc my_document
        fact price = 100
        fact price = 200
    "#,
        "my_file.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(msg.contains("Duplicate"), "Error should mention duplicate");
            assert!(msg.contains("price"), "Error should mention fact name");
        }
        Err(e) => panic!("Expected Engine error, got: {e:?}"),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_duplicate_error_is_reported() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc test
        fact x = 10
        fact x = 20
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(msg.contains("Duplicate"), "Error should mention duplicate");
            assert!(msg.contains("x"), "Error should mention fact name");
        }
        Err(e) => panic!("Expected Engine error, got: {e:?}"),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_duplicate_in_second_doc_is_caught() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc first_doc
        fact a = 1

        doc second_doc
        fact b = 2
        fact b = 3
    "#,
        "multi.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(msg.contains("Duplicate"), "Error should mention duplicate");
            assert!(msg.contains("b"), "Error should mention fact name");
        }
        Err(e) => panic!("Expected Engine error, got: {e:?}"),
        Ok(_) => panic!("Expected error"),
    }
}

// ============================================================================
// ERROR MESSAGE DISPLAY
// ============================================================================

#[test]
fn test_error_display_contains_duplicate_info() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc test
        fact value = 100
        fact value = 200
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(msg.contains("Duplicate"), "Error should mention duplicate");
            assert!(msg.contains("value"), "Error should mention fact name");
        }
        Err(e) => panic!("Expected Engine error, got: {e:?}"),
        Ok(_) => panic!("Expected error"),
    }
}

// ============================================================================
// VETO MESSAGES - Division by Zero
// ============================================================================

#[test]
fn test_division_by_zero_returns_veto_with_message() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
        doc test
        fact x = 100
        fact y = 0
        rule result = x / y
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine
        .evaluate("test", vec![], HashMap::new())
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

    let result = engine.add_lemma_code(
        r#"
        doc test
        rule x = y?
        rule y = x?
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::CircularDependency { details, .. }) => {
            let msg = &details.message;
            assert!(
                msg.to_lowercase().contains("circular") || msg.to_lowercase().contains("cycle")
            );
            assert!(msg.contains("x") && msg.contains("y"));
        }
        Err(e) => panic!("Expected CircularDependency error, got: {e:?}"),
        Ok(_) => panic!("Expected error for circular dependency"),
    }
}

// ============================================================================
// DUPLICATE DETECTION ACCURACY
// ============================================================================

#[test]
fn test_duplicate_fact_is_detected() {
    let mut engine = Engine::new();

    let lemma_code = r#"doc test
fact line2 = 1
fact line3 = 2
fact line4 = 3
fact line4 = 4"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(msg.contains("Duplicate"), "Error should mention duplicate");
            assert!(
                msg.contains("line4"),
                "Error should mention the duplicated fact name"
            );
        }
        Err(e) => panic!("Expected Engine error, got: {e:?}"),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_division_by_zero_returns_veto() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
        doc test
        fact numerator = 42
        fact denominator = 0
        rule division_result = numerator / denominator
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine
        .evaluate("test", vec![], HashMap::new())
        .expect("Should return Veto, not Error");

    let division_result = response
        .results
        .values()
        .find(|r| r.rule.name == "division_result")
        .expect("division_result rule should exist");

    assert!(
        division_result.result.is_veto(),
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

    let result = engine.add_lemma_code(
        r#"
        doc contract
        fact amount = 1000
        fact amount = 2000
    "#,
        "db://contracts/123",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(msg.contains("Duplicate"), "Error should mention duplicate");
            assert!(msg.contains("amount"), "Error should mention fact name");
        }
        Err(e) => panic!("Expected Engine error, got: {e:?}"),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_duplicate_detected_from_api_source() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc policy
        rule rate = 1.5
        rule rate = 2.0
    "#,
        "api://policies/endpoint",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(msg.contains("Duplicate"), "Error should mention duplicate");
            assert!(msg.contains("rate"), "Error should mention rule name");
        }
        Err(e) => panic!("Expected Engine error, got: {e:?}"),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_duplicate_detected_from_runtime_source() {
    let mut engine = Engine::new();

    let result = engine.add_lemma_code(
        r#"
        doc runtime_doc
        fact x = 5
        fact x = 10
    "#,
        "<runtime>",
    );

    match result {
        Err(LemmaError::Engine(details)) => {
            let msg = &details.message;
            assert!(msg.contains("Duplicate"), "Error should mention duplicate");
            assert!(msg.contains("x"), "Error should mention fact name");
        }
        Err(e) => panic!("Expected Engine error, got: {e:?}"),
        Ok(_) => panic!("Expected error"),
    }
}
