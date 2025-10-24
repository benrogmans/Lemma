use lemma::{Engine, LemmaError};

/// Test suite for error messages as documented in ERROR_MESSAGES_IMPLEMENTATION.md
/// Covers parse errors, semantic errors, and runtime errors with proper span tracking

// ============================================================================
// SEMANTIC ERRORS - Duplicate Definitions
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
        Err(LemmaError::Semantic(details)) => {
            assert!(details.message.contains("Duplicate fact definition"));
            assert!(details.message.contains("salary"));
            assert_eq!(details.doc_name, "test");
            assert_eq!(details.source_id, "test.lemma");
            assert!(details.span.line >= 3);
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
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
        Err(LemmaError::Semantic(details)) => {
            assert!(details.message.contains("Duplicate rule definition"));
            assert!(details.message.contains("total"));
            assert_eq!(details.doc_name, "test");
            assert_eq!(details.source_id, "test.lemma");
            assert!(details.span.line >= 4);
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
        Ok(_) => panic!("Expected error for duplicate rule"),
    }
}

#[test]
fn test_duplicate_fact_shows_both_locations() {
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
        Err(LemmaError::Semantic(details)) => {
            assert!(details.message.contains("Duplicate fact definition"));
            assert!(details.message.contains("name"));
            // Suggestion should mention where first definition was
            if let Some(ref sugg) = details.suggestion {
                assert!(sugg.contains("already defined"));
            }
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
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
        Some("test.lemma".to_string()),
    );

    match result {
        Err(LemmaError::Parse(details)) => {
            assert_eq!(details.source_id, "test.lemma");
            assert_eq!(details.doc_name, "<parse-error>");
        }
        Err(e) => panic!("Expected Parse error, got: {:?}", e),
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
        Some("test.lemma".to_string()),
    );

    assert!(result.is_err(), "Should fail on malformed input");

    match result {
        Err(LemmaError::Parse { .. }) => {
            // Expected
        }
        Err(e) => panic!("Expected Parse error, got: {:?}", e),
        Ok(_) => panic!("Expected parse error"),
    }
}

// ============================================================================
// RUNTIME ERRORS - Division by Zero
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

    let result = engine.evaluate("test", None, None);

    match result {
        Err(LemmaError::Runtime(details)) => {
            assert!(
                details.message.contains("division")
                    || details.message.contains("zero")
                    || details.message.contains("error"),
                "Error message should mention division or zero, got: {}",
                details.message
            );
            assert_eq!(details.doc_name, "test");
            assert_eq!(details.source_id, "test.lemma");
            assert!(details.span.line >= 4);

            // Should have a helpful suggestion
            if let Some(ref sugg) = details.suggestion {
                assert!(sugg.contains("zero") || sugg.contains("guard") || sugg.contains("check"));
            }
        }
        Err(e) => panic!("Expected Runtime error, got: {:?}", e),
        Ok(_) => panic!("Expected runtime error for division by zero"),
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

    let facts = lemma::parse_facts(&["hours_worked=0"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));

    match result {
        Err(LemmaError::Runtime(details)) => {
            assert!(
                details.message.contains("division")
                    || details.message.contains("zero")
                    || details.message.contains("error")
            );
            assert_eq!(details.doc_name, "test");
        }
        Err(e) => panic!("Expected Runtime error, got: {:?}", e),
        Ok(_) => panic!("Expected runtime error for division by zero"),
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
    let result = engine.add_lemma_code(
        r#"
        doc test
        rule x = x? + 1
    "#,
        "test.lemma",
    );

    match result {
        Err(LemmaError::CircularDependency(msg)) => {
            assert!(
                msg.to_lowercase().contains("circular") || msg.to_lowercase().contains("itself")
            );
            assert!(msg.contains("x"));
        }
        Err(e) => panic!("Expected CircularDependency error, got: {:?}", e),
        Ok(_) => panic!("Expected error for self-referencing rule"),
    }
}

// ============================================================================
// RUNTIME ERRORS - Type Mismatches
// ============================================================================

#[test]
fn test_runtime_error_type_mismatch_text_in_arithmetic() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
        doc test
        fact name = "Alice"
        fact salary = 50000
        rule result = salary + name
    "#,
            "test.lemma",
        )
        .unwrap();

    let result = engine.evaluate("test", None, None);

    match result {
        Err(LemmaError::Runtime(details)) => {
            assert!(
                details.message.contains("type")
                    || details.message.contains("error")
                    || details.message.contains("mismatch")
            );

            if let Some(ref sugg) = details.suggestion {
                assert!(!sugg.is_empty());
            }
        }
        Err(e) => panic!("Expected Runtime error for type mismatch, got: {:?}", e),
        Ok(_) => panic!("Expected runtime error for type mismatch"),
    }
}

#[test]
fn test_runtime_error_boolean_in_arithmetic() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
        doc test
        fact is_active = true
        fact count = 10
        rule result = count * is_active
    "#,
            "test.lemma",
        )
        .unwrap();

    let result = engine.evaluate("test", None, None);

    match result {
        Err(LemmaError::Runtime(details)) => {
            assert!(details.message.contains("type") || details.message.contains("error"));
        }
        Err(e) => panic!("Expected Runtime error for type mismatch, got: {:?}", e),
        Ok(_) => panic!("Expected runtime error for type mismatch"),
    }
}

// ============================================================================
// ERROR MESSAGE FORMATTING - Spans and Line Numbers
// ============================================================================

#[test]
fn test_error_contains_doc_name_and_source() {
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
        Err(LemmaError::Semantic(details)) => {
            assert_eq!(details.doc_name, "my_document");
            assert_eq!(details.source_id, "my_file.lemma");
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_error_has_valid_span() {
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
        Err(LemmaError::Semantic(details)) => {
            assert!(details.span.line > 0, "Line number should be positive");
            assert!(details.span.col > 0, "Column number should be positive");
            assert!(
                details.span.start < details.span.end,
                "Start should be before end"
            );
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_error_with_doc_start_line() {
    let mut engine = Engine::new();

    // Simulate multi-doc file - second doc starts at line 5
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
        Err(LemmaError::Semantic(details)) => {
            assert_eq!(details.doc_name, "second_doc");
            assert!(details.doc_start_line >= 1);
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}

// ============================================================================
// ERROR MESSAGE DISPLAY - Ariadne Formatting
// ============================================================================

#[test]
fn test_error_display_format() {
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
        Err(e) => {
            let error_str = format!("{}", e);

            // Should contain ariadne-formatted output
            assert!(error_str.contains("Error:") || error_str.contains("error:"));
            assert!(error_str.contains("test.lemma"));
            assert!(error_str.contains("value"));

            // Should have some formatting (colors/boxes represented in output)
            assert!(error_str.len() > 100, "Error message should be detailed");
        }
        Ok(_) => panic!("Expected error"),
    }
}

// ============================================================================
// RUNTIME ERROR SUGGESTIONS
// ============================================================================

#[test]
fn test_division_by_zero_has_helpful_suggestion() {
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

    let result = engine.evaluate("test", None, None);

    match result {
        Err(LemmaError::Runtime(details)) => {
            let sugg = details
                .suggestion
                .as_ref()
                .expect("Should have suggestion for division by zero");
            assert!(sugg.contains("zero") || sugg.contains("guard") || sugg.contains("unless"));
        }
        Err(e) => panic!("Expected Runtime error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
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
        Err(LemmaError::CircularDependency(msg)) => {
            assert!(
                msg.to_lowercase().contains("circular") || msg.to_lowercase().contains("cycle")
            );
            assert!(msg.contains("x") && msg.contains("y"));
        }
        Err(e) => panic!("Expected CircularDependency error, got: {:?}", e),
        Ok(_) => panic!("Expected error for circular dependency"),
    }
}

// ============================================================================
// SOURCE LOCATION ACCURACY
// ============================================================================

#[test]
fn test_error_points_to_correct_line() {
    let mut engine = Engine::new();

    let lemma_code = r#"doc test
fact line2 = 1
fact line3 = 2
fact line4 = 3
fact line4 = 4"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");

    match result {
        Err(LemmaError::Semantic(details)) => {
            // The duplicate 'line4' is on line 5 (1-based)
            assert_eq!(
                details.span.line, 5,
                "Should point to line 5 where duplicate is"
            );
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_runtime_error_has_source_context() {
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

    let result = engine.evaluate("test", None, None);

    match result {
        Err(LemmaError::Runtime(details)) => {
            assert!(
                !details.source_text.is_empty(),
                "Should include source text"
            );
            assert!(details.source_text.contains("numerator"));
            assert!(details.source_text.contains("denominator"));
        }
        Err(e) => panic!("Expected Runtime error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}

// ============================================================================
// NON-FILE SOURCES
// ============================================================================

#[test]
fn test_error_with_database_source() {
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
        Err(LemmaError::Semantic(details)) => {
            assert_eq!(details.source_id, "db://contracts/123");
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_error_with_api_source() {
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
        Err(LemmaError::Semantic(details)) => {
            assert_eq!(details.source_id, "api://policies/endpoint");
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_error_with_runtime_source() {
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
        Err(LemmaError::Semantic(details)) => {
            assert_eq!(details.source_id, "<runtime>");
        }
        Err(e) => panic!("Expected Semantic error, got: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}
