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

    let result = engine.load(code, SourceType::Labeled("test.lemma"));
    assert!(result.is_err(), "Engine should reject invalid parent types");

    let load_err = result.unwrap_err();
    assert!(!load_err.errors.is_empty(), "expected at least one error");
    let msg = load_err.errors[0].to_string();
    assert!(
        msg.contains("Unknown type: 'nonexistent'"),
        "Error should mention unknown type. Got: {}",
        msg
    );
}

#[test]
fn unknown_type_used_in_data_type_declaration_should_be_rejected() {
    let mut engine = Engine::new();
    let code = r#"
spec test
data value: invalid_parent_type
rule result: value
"#;

    let result = engine.load(code, SourceType::Labeled("test.lemma"));
    assert!(
        result.is_err(),
        "Engine should reject unknown types used in type declarations"
    );

    let load_err = result.unwrap_err();
    assert!(!load_err.errors.is_empty(), "expected at least one error");
    let msg = load_err.errors[0].to_string();
    assert!(
        msg.contains("Unknown type: 'invalid_parent_type'"),
        "Error should mention unknown type. Got: {}",
        msg
    );
}

#[test]
fn duplicate_spec_names_should_be_rejected() {
    let mut engine = Engine::new();
    let code = r#"
spec test
data x: 1

spec test
data x: 2
"#;

    let result = engine.load(code, SourceType::Labeled("test.lemma"));
    assert!(
        result.is_err(),
        "Duplicate spec names should be rejected (no silent overwrites)"
    );
    let load_err = result.unwrap_err();
    assert!(!load_err.errors.is_empty(), "expected at least one error");
    let msg = load_err.errors[0].to_string();
    assert!(
        msg.contains("Duplicate spec") && msg.contains("test"),
        "Error should mention the duplicate spec name. Got: {}",
        msg
    );
}
