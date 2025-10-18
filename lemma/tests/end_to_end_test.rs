use lemma::*;
use rust_decimal::Decimal;
use std::str::FromStr;

#[test]
fn test_end_to_end_simple_rule() {
    let code = r#"
doc test

fact quantity = 25

rule discount = 0
  unless quantity >= 10 then 10
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // Query the discount rule
    let response = engine.evaluate("test", vec![]).unwrap();
    let discount_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "discount")
        .unwrap();

    println!("Response: {:?}", discount_result);

    // Since quantity=25 is >= 10, we should get 10
    match &discount_result.result {
        Some(LiteralValue::Number(n)) => assert_eq!(*n, Decimal::from_str("10").unwrap()),
        _ => panic!("Expected number result"),
    }
}

#[test]
fn test_end_to_end_boolean_rule() {
    let code = r#"
doc test

fact age = 25
fact has_license = true

rule can_drive = age >= 18 and has_license
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![]).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "can_drive")
        .unwrap();

    println!("Boolean Response: {:?}", result);

    match &result.result {
        Some(LiteralValue::Boolean(b)) => assert!(*b),
        _ => panic!("Expected boolean result, got {:?}", result.result),
    }
}

#[test]
fn test_end_to_end_arithmetic() {
    let code = r#"
doc test

fact base = 100
fact multiplier = 2

rule result = base * multiplier
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![]).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "result")
        .unwrap();

    println!("Arithmetic Response: {:?}", result);

    match &result.result {
        Some(LiteralValue::Number(n)) => assert_eq!(*n, Decimal::from_str("200").unwrap()),
        _ => panic!("Expected number result, got {:?}", result.result),
    }
}

#[test]
fn test_end_to_end_rule_reference() {
    let code = r#"
doc test

fact quantity = 25

rule discount = 0
  unless quantity >= 10 then 10

rule final_price = 100 - discount?
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![]).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "final_price")
        .unwrap();

    println!("Rule Reference Response: {:?}", result);

    match &result.result {
        Some(LiteralValue::Number(n)) => assert_eq!(*n, Decimal::from_str("90").unwrap()),
        _ => panic!("Expected number result, got {:?}", result.result),
    }
}
