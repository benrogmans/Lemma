use crate::engine::Engine;
use crate::SourceType;

#[test]
fn invalid_parent_type_in_type_definition_should_be_rejected() {
    let mut engine = Engine::new();
    let code = r#"
spec test
data invalid: nonexistent -> minimum 0
data value: invalid
rule result: value
"#;

    let result = engine.load([(
        SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test.lemma"))),
        code.to_string(),
    )]);
    assert!(result.is_err(), "Engine should reject invalid parent types");

    let load_err = result.unwrap_err();
    assert!(!load_err.errors.is_empty(), "expected at least one error");
    let msg = load_err.errors[0].to_string();
    assert!(
        msg.contains("Unknown parent 'nonexistent'"),
        "Error should mention unknown type. Got: {}",
        msg
    );
}

#[test]
fn unknown_type_used_in_data_definition_should_be_rejected() {
    let mut engine = Engine::new();
    let code = r#"
spec test
data value: invalid_parent_type
rule result: value
"#;

    let result = engine.load([(
        SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test.lemma"))),
        code.to_string(),
    )]);
    assert!(
        result.is_err(),
        "Engine should reject unknown types used in type declarations"
    );

    let load_err = result.unwrap_err();
    assert!(!load_err.errors.is_empty(), "expected at least one error");
    let msg = load_err.errors[0].to_string();
    assert!(
        msg.contains("Unknown parent 'invalid_parent_type'"),
        "Error should mention unknown type. Got: {}",
        msg
    );
}

#[test]
fn duplicate_spec_versions_same_effective_should_be_rejected() {
    let mut engine = Engine::new();
    let code = r#"
spec test
data x: 1

spec test
data x: 2
"#;
    let path = std::sync::Arc::new(std::path::PathBuf::from("test.lemma"));

    let result = engine.load([(SourceType::Path(path.clone()), code.to_string())]);
    assert!(
        result.is_err(),
        "Duplicate spec rows for same identity should be rejected (no silent overwrites)"
    );
    let load_err = result.unwrap_err();
    assert_eq!(
        load_err.errors.len(),
        2,
        "expected errors on both duplicate declarations, got: {:?}",
        load_err.errors
    );
    for err in &load_err.errors {
        let msg = err.to_string();
        assert!(
            msg.contains("Duplicate spec") && msg.contains("test"),
            "Error should mention the duplicate spec name. Got: {}",
            msg
        );
        assert_eq!(
            err.location().map(|s| s.source_type.to_string()),
            Some(path.to_string_lossy().to_string()),
            "both errors must attribute to the declaring file"
        );
    }
}

#[test]
fn duplicate_spec_across_two_files_reports_both_paths() {
    let mut engine = Engine::new();
    let path_a = std::sync::Arc::new(std::path::PathBuf::from("file_a.lemma"));
    let path_b = std::sync::Arc::new(std::path::PathBuf::from("file_b.lemma"));

    let result = engine.load([
        (
            SourceType::Path(path_a.clone()),
            "spec dup\ndata x: 1".to_string(),
        ),
        (
            SourceType::Path(path_b.clone()),
            "spec dup\ndata x: 2".to_string(),
        ),
    ]);
    assert!(
        result.is_err(),
        "cross-file duplicate spec must be rejected"
    );
    let load_err = result.unwrap_err();
    assert_eq!(
        load_err.errors.len(),
        2,
        "expected errors on both declaring files, got: {:?}",
        load_err.errors
    );
    let paths: Vec<String> = load_err
        .errors
        .iter()
        .map(|err| {
            err.location()
                .expect("duplicate errors must have source")
                .source_type
                .to_string()
        })
        .collect();
    assert!(
        paths.contains(&path_a.to_string_lossy().to_string()),
        "expected error on file_a, got paths: {paths:?}"
    );
    assert!(
        paths.contains(&path_b.to_string_lossy().to_string()),
        "expected error on file_b, got paths: {paths:?}"
    );
    let joined = load_err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("also declared"),
        "expected cross-file location in message, got: {joined}"
    );
}
