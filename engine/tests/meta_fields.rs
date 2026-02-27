mod common;
use common::add_lemma_code_blocking;
use lemma::Engine;

#[test]
fn test_meta_fields_parsing_and_planning() {
    let mut engine = Engine::new();
    let code = r#"
doc meta_test

meta title: "Test Document"
meta version: v1.2.3
meta from: 2025-01-01
meta to: 2025-12-31
meta author: "Alice"

fact x: 1
"#;

    add_lemma_code_blocking(&mut engine, code, "meta_test.lemma")
        .expect("Failed to parse meta_test");

    let plan = engine
        .get_execution_plan("meta_test")
        .expect("Plan not found");

    // Check meta fields
    // Note: Display for Text literal is unquoted in Value::Display
    assert_eq!(
        plan.meta.get("title").map(|v| v.to_string()),
        Some("Test Document".to_string())
    );
    assert_eq!(
        plan.meta.get("version").map(|v| v.to_string()),
        Some("v1.2.3".to_string())
    );
    assert_eq!(
        plan.meta.get("from").map(|v| v.to_string()),
        Some("2025-01-01".to_string())
    );
    assert_eq!(
        plan.meta.get("to").map(|v| v.to_string()),
        Some("2025-12-31".to_string())
    );
    assert_eq!(
        plan.meta.get("author").map(|v| v.to_string()),
        Some("Alice".to_string())
    );
}

#[test]
fn test_meta_fields_validation_errors() {
    let mut engine = Engine::new();
    let code = r#"
doc meta_error

meta title: 123
meta from: "not a date"
"#;

    let result = add_lemma_code_blocking(&mut engine, code, "meta_error.lemma");
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Meta 'title' must be a text literal"));
    assert!(err_msg.contains("Meta 'from' must be a date literal"));
}

#[test]
fn test_duplicate_meta_key() {
    let mut engine = Engine::new();
    let code = r#"
doc meta_dup

meta title: "First"
meta title: "Second"
"#;

    let result = add_lemma_code_blocking(&mut engine, code, "meta_dup.lemma");
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Duplicate meta key 'title'"));
}
