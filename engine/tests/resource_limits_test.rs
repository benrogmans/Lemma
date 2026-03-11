use lemma::{Engine, Error, ResourceLimits};
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use std::time::Instant;

#[test]
fn test_file_size_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 100,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);

    // Create a file larger than 100 bytes
    let large_code = "spec test\nfact x: 1\n".repeat(10); // ~200 bytes

    let result = add_lemma_code_blocking(&mut engine, &large_code, "test.lemma");

    let errs = result.unwrap_err();
    let limit_err =
        find_resource_limit_name(&errs).expect("expected at least one ResourceLimitExceeded");
    assert_eq!(limit_err, "max_file_size_bytes");
}

#[test]
fn test_file_size_just_under_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 1000,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);
    let code = "spec test fact x: 1 rule y: x + 1"; // Small file

    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(result.is_ok(), "Small file should be accepted");
}

#[test]
fn test_expression_depth_limit() {
    let limits = ResourceLimits::default();
    assert_eq!(limits.max_expression_depth, 5);

    let mut engine = Engine::with_limits(limits);
    let code_4 = r#"spec test
fact x: 1
rule r: (((1 + 1) + 1) + 1) + 1"#;
    let result = add_lemma_code_blocking(&mut engine, code_4, "test.lemma");
    assert!(
        result.is_ok(),
        "Depth 4 should be accepted: {:?}",
        result.err()
    );
}

#[test]
fn test_overall_execution_time_at_expression_depth_limit() {
    let limits = ResourceLimits::default();
    let code_4 = r#"spec test
fact x: 1
rule r: (((1 + 1) + 1) + 1) + 1"#;
    let mut engine = Engine::with_limits(limits);
    let start = Instant::now();
    add_lemma_code_blocking(&mut engine, code_4, "test.lemma").expect("add_lemma_files");
    let now = DateTimeValue::now();
    let _ = engine
        .evaluate(
            "test",
            None,
            &now,
            vec!["r".to_string()],
            std::collections::HashMap::new(),
        )
        .expect("evaluate");
    let elapsed = start.elapsed();
    eprintln!("overall (parse + plan + evaluate, depth 4): {:?}", elapsed);
}

#[test]
fn test_fact_value_size_limit() {
    let limits = ResourceLimits {
        max_fact_value_bytes: 50,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);
    add_lemma_code_blocking(
        &mut engine,
        "spec test\nfact name: [text]\nrule result: name",
        "test.lemma",
    )
    .unwrap();

    let large_string = "a".repeat(100);
    let mut facts = std::collections::HashMap::new();
    facts.insert("name".to_string(), large_string);

    let now = DateTimeValue::now();
    let result = engine.evaluate("test", None, &now, vec![], facts);

    match result {
        Err(Error::ResourceLimitExceeded { ref limit_name, .. }) => {
            assert_eq!(limit_name, "max_fact_value_bytes");
        }
        _ => panic!("Expected ResourceLimitExceeded error for large fact value"),
    }
}

// --- Name length limits ---

/// Helper to extract the `limit_name` from the first `ResourceLimitExceeded` in a list of errors.
fn find_resource_limit_name(errors: &[Error]) -> Option<String> {
    errors.iter().find_map(|e| match e {
        Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.clone()),
        _ => None,
    })
}

#[test]
fn spec_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_SPEC_NAME_LENGTH);
    let code = format!("spec {name}\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Spec name at max length should be accepted: {result:?}"
    );
}

#[test]
fn spec_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_SPEC_NAME_LENGTH + 1);
    let code = format!("spec {name}\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err =
        find_resource_limit_name(&errs).expect("expected ResourceLimitExceeded for spec name");
    assert_eq!(limit_err, "max_spec_name_length");
}

#[test]
fn fact_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH);
    let code = format!("spec test\nfact {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Fact name at max length should be accepted: {result:?}"
    );
}

#[test]
fn fact_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH + 1);
    let code = format!("spec test\nfact {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err =
        find_resource_limit_name(&errs).expect("expected ResourceLimitExceeded for fact name");
    assert_eq!(limit_err, "max_fact_name_length");
}

#[test]
fn fact_binding_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH + 1);
    let code = format!("spec test\nfact other.{name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err = find_resource_limit_name(&errs)
        .expect("expected ResourceLimitExceeded for fact binding name");
    assert_eq!(limit_err, "max_fact_name_length");
}

#[test]
fn rule_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_RULE_NAME_LENGTH);
    let code = format!("spec test\nrule {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Rule name at max length should be accepted: {result:?}"
    );
}

#[test]
fn rule_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_RULE_NAME_LENGTH + 1);
    let code = format!("spec test\nrule {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err =
        find_resource_limit_name(&errs).expect("expected ResourceLimitExceeded for rule name");
    assert_eq!(limit_err, "max_rule_name_length");
}

#[test]
fn type_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH);
    let code = format!("spec test\ntype {name}: number\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Type name at max length should be accepted: {result:?}"
    );
}

#[test]
fn type_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH + 1);
    let code = format!("spec test\ntype {name}: number\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let rle =
        find_resource_limit_name(&errs).expect("expected ResourceLimitExceeded for type name");
    assert_eq!(rle, "max_type_name_length");
}

#[test]
fn type_import_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH + 1);
    let code = format!("spec test\ntype {name} from other\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let rle = find_resource_limit_name(&errs)
        .expect("expected ResourceLimitExceeded for type import name");
    assert_eq!(rle, "max_type_name_length");
}
