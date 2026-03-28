use lemma::parsing::ast::DateTimeValue;
/// Comprehensive tests for fact binding type validation
///
/// These tests ensure that the engine correctly validates that fact bindings
/// match the expected types declared in the spec, preventing type confusion bugs.
use lemma::Engine;
use std::collections::HashMap;

#[test]
fn test_number_type_validation_rejects_text() {
    let code = r#"
spec test
fact age: [number]
rule doubled: age * 2
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut facts = HashMap::new();
    facts.insert("age".to_string(), "twenty".to_string());

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), facts, false);

    assert!(result.is_err(), "Expected error but got: {:?}", result);
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Failed to parse fact 'age'"),
        "Error was: {}",
        error
    );
}

#[test]
fn test_multiple_type_validations() {
    let code = r#"
spec test
fact price: [number]
fact quantity: [number]
fact active: [boolean]
rule total: price * quantity
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "expensive".to_string());
    facts.insert("quantity".to_string(), "5".to_string());
    facts.insert("active".to_string(), "true".to_string());

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), facts, false);
    assert!(result.is_err(), "Expected type mismatch error");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Failed to parse fact 'price'"));

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "100".to_string());
    facts.insert("quantity".to_string(), "five".to_string());
    facts.insert("active".to_string(), "true".to_string());

    let err = engine
        .run("test", Some(&now), facts, false)
        .expect_err("quantity must reject non-number");
    assert!(err.to_string().contains("Failed to parse fact 'quantity'"));

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "100".to_string());
    facts.insert("quantity".to_string(), "5".to_string());
    facts.insert("active".to_string(), "maybe".to_string());

    let err = engine
        .run("test", Some(&now), facts, false)
        .expect_err("active must reject non-boolean");
    assert!(err.to_string().contains("Failed to parse fact 'active'"));

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "100".to_string());
    facts.insert("quantity".to_string(), "5".to_string());
    facts.insert("active".to_string(), "true".to_string());

    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("valid facts must evaluate");
    let total = response
        .results
        .get("total")
        .expect("total rule")
        .result
        .value()
        .expect("total value");
    assert_eq!(total.to_string(), "500");
}

#[test]
fn test_literal_fact_type_validation() {
    let code = r#"
spec test
fact base_price: 50
rule total: base_price * 1.2
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut facts = HashMap::new();
    facts.insert("base_price".to_string(), "sixty".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("test", Some(&now), facts, false)
        .expect_err("base_price must reject non-number");
    assert!(err
        .to_string()
        .contains("Failed to parse fact 'base_price'"));

    let mut facts = HashMap::new();
    facts.insert("base_price".to_string(), "60".to_string());

    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("valid base_price must evaluate");
    let total = response
        .results
        .get("total")
        .expect("total rule")
        .result
        .value()
        .expect("total value");
    assert!(
        total.to_string().starts_with("72"),
        "60 * 1.2 = 72, got {}",
        total
    );
}

#[test]
fn test_unknown_fact_binding_rejected() {
    let code = r#"
spec test
fact price: [number]
rule total: price * 1.1
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "100".to_string());
    facts.insert("unknown_fact".to_string(), "42".to_string());

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), facts, false);
    assert!(result.is_err(), "Expected error for unknown fact binding");
    assert!(result.unwrap_err().to_string().contains("unknown_fact"));
}

#[test]
fn test_fact_binding_with_type_definition_should_fail() {
    let code = r#"
spec base
fact quantity: [number -> minimum 0 -> default 10]
rule total: quantity * 2

spec test
fact line: spec base
fact line.quantity: [number -> minimum 0 -> default 5]
rule result: line.total
"#;

    let mut engine = Engine::new();
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));

    assert!(
        result.is_err(),
        "Expected error when overriding typed fact with type definition"
    );

    let errs = result.unwrap_err();
    let error_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        error_msg.contains("quantity")
            || error_msg.contains("type")
            || error_msg.contains("binding"),
        "Error message should mention the problematic fact or type binding. Got: {}",
        error_msg
    );
}
