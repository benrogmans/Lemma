use crate::engine::Engine;
use crate::parsing::ast::DateTimeValue;
use crate::Error;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn add_lemma_code(engine: &mut Engine, code: &str, source: &str) -> Result<(), Vec<Error>> {
    engine.load(code, crate::LoadSource::Labeled(source))
}

#[test]
fn test_evaluate_spec_all_rules() {
    let mut engine = Engine::new();
    add_lemma_code(
        &mut engine,
        r#"
        spec test
        fact x: 10
        fact y: 5
        rule sum: x + y
        rule product: x * y
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
    assert_eq!(response.results.len(), 2);

    let sum_result = response
        .results
        .values()
        .find(|r| r.rule.name == "sum")
        .unwrap();
    assert_eq!(
        sum_result.result,
        crate::OperationResult::Value(Box::new(crate::LiteralValue::number(
            Decimal::from_str("15").unwrap()
        )))
    );

    let product_result = response
        .results
        .values()
        .find(|r| r.rule.name == "product")
        .unwrap();
    assert_eq!(
        product_result.result,
        crate::OperationResult::Value(Box::new(crate::LiteralValue::number(
            Decimal::from_str("50").unwrap()
        )))
    );
}

#[test]
fn test_evaluate_empty_facts() {
    let mut engine = Engine::new();
    add_lemma_code(
        &mut engine,
        r#"
        spec test
        fact price: 100
        rule total: price * 2
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(
        response.results.values().next().unwrap().result,
        crate::OperationResult::Value(Box::new(crate::LiteralValue::number(
            Decimal::from_str("200").unwrap()
        )))
    );
}

#[test]
fn test_evaluate_boolean_rule() {
    let mut engine = Engine::new();
    add_lemma_code(
        &mut engine,
        r#"
        spec test
        fact age: 25
        rule is_adult: age >= 18
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
    assert_eq!(
        response.results.values().next().unwrap().result,
        crate::OperationResult::Value(Box::new(crate::LiteralValue::from_bool(true)))
    );
}

#[test]
fn test_evaluate_with_unless_clause() {
    let mut engine = Engine::new();
    add_lemma_code(
        &mut engine,
        r#"
        spec test
        fact quantity: 15
        rule discount: 0
          unless quantity >= 10 then 10
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
    assert_eq!(
        response.results.values().next().unwrap().result,
        crate::OperationResult::Value(Box::new(crate::LiteralValue::number(
            Decimal::from_str("10").unwrap()
        )))
    );
}

#[test]
fn test_spec_not_found() {
    let engine = Engine::new();
    let now = DateTimeValue::now();
    let result = engine.run("nonexistent", Some(&now), HashMap::new());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(
        err.to_string(),
        "Request error: Spec 'nonexistent' not found"
    );
}

#[test]
fn test_multiple_specs() {
    let mut engine = Engine::new();
    add_lemma_code(
        &mut engine,
        r#"
        spec spec1
        fact x: 10
        rule result: x * 2
    "#,
        "spec1.lemma",
    )
    .unwrap();

    add_lemma_code(
        &mut engine,
        r#"
        spec spec2
        fact y: 5
        rule result: y * 3
    "#,
        "spec2.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response1 = engine.run("spec1", Some(&now), HashMap::new()).unwrap();
    assert_eq!(
        response1.results.values().next().unwrap().result,
        crate::OperationResult::Value(Box::new(crate::LiteralValue::number(
            Decimal::from_str("20").unwrap()
        )))
    );

    let response2 = engine.run("spec2", Some(&now), HashMap::new()).unwrap();
    assert_eq!(
        response2.results.values().next().unwrap().result,
        crate::OperationResult::Value(Box::new(crate::LiteralValue::number(
            Decimal::from_str("15").unwrap()
        )))
    );
}

#[test]
fn test_runtime_error_mapping() {
    let mut engine = Engine::new();
    add_lemma_code(
        &mut engine,
        r#"
        spec test
        fact numerator: 10
        fact denominator: 0
        rule division: numerator / denominator
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), HashMap::new());
    // Division by zero returns a Veto (not an error)
    assert!(result.is_ok(), "Evaluation should succeed");
    let response = result.unwrap();
    let division_result = response
        .results
        .values()
        .find(|r| r.rule.name == "division");
    assert!(
        division_result.is_some(),
        "Should have division rule result"
    );
    match &division_result.unwrap().result {
        crate::OperationResult::Veto(message) => {
            assert!(
                message
                    .as_ref()
                    .map(|m| m.contains("Division by zero"))
                    .unwrap_or(false),
                "Veto message should mention division by zero: {:?}",
                message
            );
        }
        other => panic!("Expected Veto for division by zero, got {:?}", other),
    }
}

#[test]
fn test_rules_sorted_by_source_order() {
    let mut engine = Engine::new();
    add_lemma_code(
        &mut engine,
        r#"
        spec test
        fact a: 1
        fact b: 2
        rule z: a + b
        rule y: a * b
        rule x: a - b
    "#,
        "test.lemma",
    )
    .unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
    assert_eq!(response.results.len(), 3);

    assert!(response.results.contains_key("z"));
    assert!(response.results.contains_key("y"));
    assert!(response.results.contains_key("x"));

    // Verify source positions increase (z < y < x)
    let z_pos = response
        .results
        .values()
        .find(|r| r.rule.name == "z")
        .unwrap()
        .rule
        .source_location
        .span
        .start;
    let y_pos = response
        .results
        .values()
        .find(|r| r.rule.name == "y")
        .unwrap()
        .rule
        .source_location
        .span
        .start;
    let x_pos = response
        .results
        .values()
        .find(|r| r.rule.name == "x")
        .unwrap()
        .rule
        .source_location
        .span
        .start;

    assert!(z_pos < y_pos);
    assert!(y_pos < x_pos);
}

#[test]
fn invalid_parent_type_in_type_definition_should_be_rejected() {
    let mut engine = Engine::new();
    let code = r#"
spec test
type invalid: nonexistent -> minimum 0
fact value: [invalid]
rule result: value
"#;

    let result = add_lemma_code(&mut engine, code, "test.lemma");
    assert!(result.is_err(), "Engine should reject invalid parent types");

    let errs = result.unwrap_err();
    assert!(!errs.is_empty(), "expected at least one error");
    let msg = errs[0].to_string();
    assert!(
        msg.contains("Unknown type: 'nonexistent'"),
        "Error should mention unknown type. Got: {}",
        msg
    );
}

#[test]
fn unknown_type_used_in_fact_type_declaration_should_be_rejected() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact value: [invalid_parent_type]
rule result: value
"#;

    let result = add_lemma_code(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "Engine should reject unknown types used in type declarations"
    );

    let errs = result.unwrap_err();
    assert!(!errs.is_empty(), "expected at least one error");
    let msg = errs[0].to_string();
    assert!(
        msg.contains("Unknown type: 'invalid_parent_type'"),
        "Error should mention unknown type. Got: {}",
        msg
    );
}

#[test]
fn test_rule_filtering_evaluates_dependencies() {
    let mut engine = Engine::new();
    add_lemma_code(
        &mut engine,
        r#"
        spec test
        fact base: 100
        rule subtotal: base * 2
        rule tax: subtotal * 10%
        rule total: subtotal + tax
    "#,
        "test.lemma",
    )
    .unwrap();

    // User filters to 'total' after run (deps were still computed)
    let now = DateTimeValue::now();
    let mut response = engine.run("test", Some(&now), HashMap::new()).unwrap();
    response.filter_rules(&[String::from("total")]);

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results.keys().next().unwrap(), "total");

    // But the value should be correct (dependencies were computed)
    let total = response.results.values().next().unwrap();
    assert_eq!(
        total.result,
        crate::OperationResult::Value(Box::new(crate::LiteralValue::number(
            Decimal::from_str("220").unwrap()
        )))
    );
}

#[test]
fn test_add_lemma_code_empty_string_is_ok() {
    let mut engine = Engine::new();
    let result = add_lemma_code(&mut engine, "", "test.lemma");
    assert!(result.is_ok());
    assert!(
        engine.list_specs().is_empty(),
        "Empty input should produce no specs"
    );
}

#[test]
fn test_add_lemma_code_whitespace_only_is_ok() {
    let mut engine = Engine::new();
    let result = add_lemma_code(&mut engine, "   \n\t  ", "test.lemma");
    assert!(result.is_ok());
    assert!(
        engine.list_specs().is_empty(),
        "Whitespace-only input should produce no specs"
    );
}

#[test]
fn duplicate_spec_names_should_be_rejected() {
    // Higher-standard behavior: duplicate spec names are an error (no silent overwrites).
    let mut engine = Engine::new();
    let code = r#"
spec test
fact x: 1

spec test
fact x: 2
"#;

    let result = add_lemma_code(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "Duplicate spec names should be rejected (no silent overwrites)"
    );
    let errs = result.unwrap_err();
    assert!(!errs.is_empty(), "expected at least one error");
    let msg = errs[0].to_string();
    assert!(
        msg.contains("Duplicate spec") && msg.contains("test"),
        "Error should mention the duplicate spec name. Got: {}",
        msg
    );
}
