mod common;
use common::add_lemma_code_blocking;
use lemma::{DateTimeValue, Engine};

#[test]
fn test_meta_fields_parsing_and_planning() {
    let mut engine = Engine::new();
    let code = r#"
doc meta_test 2025-01-01

meta title: "Test Document"
meta version: v1.2.3
meta author: "Alice"

fact x: 1
"#;

    add_lemma_code_blocking(&mut engine, code, "meta_test.lemma")
        .expect("Failed to parse meta_test");

    let effective = DateTimeValue {
        year: 2025,
        month: 6,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
    };
    let plan = engine
        .get_execution_plan("meta_test", None, &effective)
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
"#;

    let result = add_lemma_code_blocking(&mut engine, code, "meta_error.lemma");
    assert!(result.is_err());
    let errs = result.unwrap_err();
    let err_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(err_msg.contains("Meta 'title' must be a text literal"));
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
    let errs = result.unwrap_err();
    let err_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(err_msg.contains("Duplicate meta key 'title'"));
}

#[test]
fn test_later_doc_version_can_evolve_interface() {
    // With temporal slicing, later versions of a document CAN have different
    // facts/rules. The constraint is per-dependent-per-slice, not global.
    // Since no other document depends on pricing's "total" rule here, the
    // second version without it is valid.
    let mut engine = Engine::new();
    let code = r#"
doc pricing 2024-01-01
fact x: 10
rule total: x

doc pricing 2025-01-01
fact x: 20
"#;
    let result = add_lemma_code_blocking(&mut engine, code, "pricing.lemma");
    assert!(
        result.is_ok(),
        "later doc with different interface should be accepted when no dependent requires the old interface: {:?}",
        result.err()
    );
}
