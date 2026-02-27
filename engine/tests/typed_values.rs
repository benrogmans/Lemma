use lemma::{ValueKind, *};
mod common;
use common::add_lemma_code_blocking;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_percentage_arithmetic() {
    let code = r#"
doc pricing
fact discount: 25%
rule net_multiplier: 1 - discount
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let response = engine.evaluate("pricing", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .get("net_multiplier")
        .unwrap()
        .result
        .value()
        .unwrap();

    match result {
        LiteralValue {
            value: ValueKind::Number(n),
            ..
        } => assert_eq!(n, &Decimal::from_str("0.75").unwrap()),
        _ => panic!("Expected Number, got {:?}", result),
    }
}

#[test]
fn test_duration_operations() {
    let code = r#"
doc scheduling
fact meeting_length: 30 minutes
rule double_meeting: meeting_length * 2
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let response = engine
        .evaluate("scheduling", vec![], HashMap::new())
        .unwrap();
    let result = response
        .results
        .get("double_meeting")
        .unwrap()
        .result
        .value()
        .unwrap();

    match result {
        LiteralValue {
            value: ValueKind::Duration(value, _unit),
            ..
        } => {
            // 30 minutes * 2 = 60 (stored as the numeric value in minutes unit)
            assert_eq!(value, &Decimal::from(60));
        }
        _ => panic!("Expected Duration, got {:?}", result),
    }
}

#[test]
fn test_date_arithmetic_with_duration() {
    let code = r#"
doc dates
fact start: 2024-01-15
rule end: start + 7 days
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let response = engine.evaluate("dates", vec![], HashMap::new()).unwrap();
    let result = response.results.get("end").unwrap().result.value().unwrap();

    match result {
        LiteralValue {
            value: ValueKind::Date(dt),
            ..
        } => {
            assert_eq!(dt.year, 2024);
            assert_eq!(dt.month, 1);
            assert_eq!(dt.day, 22);
        }
        _ => panic!("Expected Date, got {:?}", result),
    }
}

#[test]
fn test_boolean_operations() {
    let code = r#"
doc logic
fact is_active: true
fact is_premium: false
rule can_access: is_active and not is_premium
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let response = engine.evaluate("logic", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .get("can_access")
        .unwrap()
        .result
        .value()
        .unwrap();

    match result {
        LiteralValue {
            value: ValueKind::Boolean(b),
            ..
        } => {
            assert!(*b);
        }
        _ => panic!("Expected Boolean, got {:?}", result),
    }
}

#[test]
fn test_text_operations() {
    let code = r#"
doc strings
fact greeting: "hello"
rule message: greeting
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let response = engine.evaluate("strings", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .get("message")
        .unwrap()
        .result
        .value()
        .unwrap();

    match result {
        LiteralValue {
            value: ValueKind::Text(s),
            ..
        } => assert_eq!(s, "hello"),
        _ => panic!("Expected Text, got {:?}", result),
    }
}
