use lemma::{DateGranularity, DateTimeValue, Engine};

#[test]
fn test_meta_fields_parsing_and_planning() {
    let mut engine = Engine::new();
    let code = r#"
spec meta_test 2025-01-01

meta title: "Test Spec"
meta version: v1.2.3
meta author: "Alice"

data x: 1
"#;

    engine
        .load([(
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "meta_test.lemma",
            ))),
            code.to_string(),
        )])
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

        granularity: DateGranularity::Full,
    };
    let show = engine
        .show(None, "meta_test", Some(&effective))
        .expect("Plan not found");

    assert_eq!(
        show.meta.get("title").map(|v| v.to_string()),
        Some("Test Spec".to_string())
    );
    assert_eq!(
        show.meta.get("version").map(|v| v.to_string()),
        Some("v1.2.3".to_string())
    );
    assert_eq!(
        show.meta.get("author").map(|v| v.to_string()),
        Some("Alice".to_string())
    );
}

#[test]
fn test_meta_fields_validation_errors() {
    let mut engine = Engine::new();
    let code = r#"
spec meta_error

meta title: 123
"#;

    let errs = engine
        .load([(
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "meta_error.lemma",
            ))),
            code.to_string(),
        )])
        .expect_err("meta title must reject non-text");
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
spec meta_dup

meta title: "First"
meta title: "Second"
"#;

    let errs = engine
        .load([(
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "meta_dup.lemma",
            ))),
            code.to_string(),
        )])
        .expect_err("duplicate meta key must fail");
    let err_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(err_msg.contains("Duplicate meta key 'title'"));
}
