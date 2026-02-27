use lemma::{Engine, Error, ResourceLimits};
mod common;
use common::add_lemma_code_blocking;

#[test]
fn test_file_size_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 100,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);

    // Create a file larger than 100 bytes
    let large_code = "doc test\nfact x: 1\n".repeat(10); // ~200 bytes

    let result = add_lemma_code_blocking(&mut engine, &large_code, "test.lemma");

    match result {
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_file_size_bytes");
        }
        _ => panic!("Expected ResourceLimitExceeded error"),
    }
}

#[test]
fn test_file_size_just_under_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 1000,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);
    let code = "doc test fact x: 1 rule y: x + 1"; // Small file

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
        "doc test\nfact name: [text]\nrule result: name",
        "test.lemma",
    )
    .unwrap();

    let large_string = "a".repeat(100);
    let mut facts = std::collections::HashMap::new();
    facts.insert("name".to_string(), large_string);

    let result = engine.evaluate("test", vec![], facts);

    match result {
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_fact_value_bytes");
        }
        _ => panic!("Expected ResourceLimitExceeded error for large fact value"),
    }
}

// --- Name length limits ---

#[test]
fn doc_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_DOC_NAME_LENGTH);
    let code = format!("doc {name}\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Doc name at max length should be accepted: {result:?}"
    );
}

#[test]
fn doc_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_DOC_NAME_LENGTH + 1);
    let code = format!("doc {name}\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    match result {
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_doc_name_length");
        }
        other => panic!("Expected ResourceLimitExceeded for doc name, got: {other:?}"),
    }
}

#[test]
fn fact_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH);
    let code = format!("doc test\nfact {name}: 1");
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
    let code = format!("doc test\nfact {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    match result {
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_fact_name_length");
        }
        other => panic!("Expected ResourceLimitExceeded for fact name, got: {other:?}"),
    }
}

#[test]
fn fact_binding_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH + 1);
    let code = format!("doc test\nfact other.{name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    match result {
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_fact_name_length");
        }
        other => panic!("Expected ResourceLimitExceeded for fact binding name, got: {other:?}"),
    }
}

#[test]
fn rule_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_RULE_NAME_LENGTH);
    let code = format!("doc test\nrule {name}: 1");
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
    let code = format!("doc test\nrule {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    match result {
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_rule_name_length");
        }
        other => panic!("Expected ResourceLimitExceeded for rule name, got: {other:?}"),
    }
}

#[test]
fn type_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH);
    let code = format!("doc test\ntype {name}: number\nfact x: 1");
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
    let code = format!("doc test\ntype {name}: number\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    match result {
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_type_name_length");
        }
        other => panic!("Expected ResourceLimitExceeded for type name, got: {other:?}"),
    }
}

#[test]
fn type_import_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH + 1);
    let code = format!("doc test\ntype {name} from other\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    match result {
        Err(Error::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_type_name_length");
        }
        other => panic!("Expected ResourceLimitExceeded for type import name, got: {other:?}"),
    }
}
