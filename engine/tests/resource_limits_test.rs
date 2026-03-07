use lemma::{Engine, Error, ResourceLimits};
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;

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
    let limit_err = errs
        .iter()
        .find_map(|e| match e {
            Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.clone()),
            _ => None,
        })
        .expect("expected at least one ResourceLimitExceeded");
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
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_fact_value_bytes");
        }
        _ => panic!("Expected ResourceLimitExceeded error for large fact value"),
    }
}

// --- Name length limits ---

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
    let limit_err = errs
        .iter()
        .find_map(|e| match e {
            Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.clone()),
            _ => None,
        })
        .expect("expected ResourceLimitExceeded for spec name");
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
    let limit_err = errs
        .iter()
        .find_map(|e| match e {
            Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.clone()),
            _ => None,
        })
        .expect("expected ResourceLimitExceeded for fact name");
    assert_eq!(limit_err, "max_fact_name_length");
}

#[test]
fn fact_binding_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH + 1);
    let code = format!("spec test\nfact other.{name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err = errs
        .iter()
        .find_map(|e| match e {
            Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.clone()),
            _ => None,
        })
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
    let limit_err = errs
        .iter()
        .find_map(|e| match e {
            Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.clone()),
            _ => None,
        })
        .expect("expected ResourceLimitExceeded for rule name");
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
    let rle = errs
        .iter()
        .find_map(|e| match e {
            Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.as_str()),
            _ => None,
        })
        .expect("expected ResourceLimitExceeded for type name");
    assert_eq!(rle, "max_type_name_length");
}

#[test]
fn type_import_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH + 1);
    let code = format!("spec test\ntype {name} from other\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let rle = errs
        .iter()
        .find_map(|e| match e {
            Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.as_str()),
            _ => None,
        })
        .expect("expected ResourceLimitExceeded for type import name");
    assert_eq!(rle, "max_type_name_length");
}
