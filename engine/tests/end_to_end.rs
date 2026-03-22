use lemma::parsing::ast::DateTimeValue;
use lemma::*;
mod common;
use common::add_lemma_code_blocking;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_end_to_end_simple_rule() {
    let code = r#"
spec test

fact quantity: 25

rule discount: 0
  unless quantity >= 10 then 10
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    // Query the discount rule
    let response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount")
        .unwrap();

    println!("Response: {:?}", discount_result);

    // Since quantity=25 is >= 10, we should get 10
    match &discount_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(n) = &lit.value {
                assert_eq!(*n, Decimal::from_str("10").unwrap());
            } else {
                panic!("Expected number result");
            }
        }
        _ => panic!("Expected number result"),
    }
}

#[test]
fn test_end_to_end_boolean_rule() {
    let code = r#"
spec test

fact age: 25
fact has_license: true

rule can_drive: age >= 18 and has_license
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    let response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "can_drive")
        .unwrap();

    println!("Boolean Response: {:?}", result);

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Boolean(b) = &lit.value {
                assert!(*b);
            } else {
                panic!("Expected boolean result, got {:?}", result.result);
            }
        }
        _ => panic!("Expected boolean result, got {:?}", result.result),
    }
}

#[test]
fn test_end_to_end_arithmetic() {
    let code = r#"
spec test

fact base: 100
fact multiplier: 2

rule result: base * multiplier
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    let response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .unwrap();

    println!("Arithmetic Response: {:?}", result);

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(n) = &lit.value {
                assert_eq!(*n, Decimal::from_str("200").unwrap());
            } else {
                panic!("Expected number result, got {:?}", result.result);
            }
        }
        _ => panic!("Expected number result, got {:?}", result.result),
    }
}

#[test]
fn test_end_to_end_rule_reference() {
    let code = r#"
spec test

fact quantity: 25

rule discount: 0
  unless quantity >= 10 then 10

rule final_price: 100 - discount
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    let response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "final_price")
        .unwrap();

    println!("Rule Reference Response: {:?}", result);

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(n) = &lit.value {
                assert_eq!(*n, Decimal::from_str("90").unwrap());
            } else {
                panic!("Expected number result, got {:?}", result.result);
            }
        }
        _ => panic!("Expected number result, got {:?}", result.result),
    }
}
