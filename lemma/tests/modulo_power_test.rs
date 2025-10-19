use lemma::*;
use rust_decimal::Decimal;
use std::str::FromStr;

#[test]
fn test_modulo_simple() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test
fact a = 10
fact b = 3
rule remainder = a % b
"#,
            "test",
        )
        .unwrap();

    let response = engine.evaluate("test", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "remainder")
        .unwrap();

    match &result.result {
        Some(LiteralValue::Number(n)) => assert_eq!(*n, Decimal::from_str("1").unwrap()),
        _ => panic!("Expected number, got {:?}", result.result),
    }
}

#[test]
fn test_power_simple() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test
fact base = 2
fact exponent = 3
rule result = base ^ exponent
"#,
            "test",
        )
        .unwrap();

    let response = engine.evaluate("test", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "result")
        .unwrap();

    match &result.result {
        Some(LiteralValue::Number(n)) => assert_eq!(*n, Decimal::from_str("8").unwrap()),
        _ => panic!("Expected number, got {:?}", result.result),
    }
}

#[test]
fn test_modulo_in_expression() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test
fact value = 17
rule is_even = (value % 2) == 0
rule is_odd = (value % 2) == 1
"#,
            "test",
        )
        .unwrap();

    let response = engine.evaluate("test", None, None).unwrap();

    let is_even = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_even")
        .unwrap();
    assert_eq!(is_even.result, Some(LiteralValue::Boolean(false)));

    let is_odd = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_odd")
        .unwrap();
    assert_eq!(is_odd.result, Some(LiteralValue::Boolean(true)));
}

#[test]
fn test_power_with_fractions() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test
fact base = 4
rule square_root = base ^ 0.5
"#,
            "test",
        )
        .unwrap();

    let response = engine.evaluate("test", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "square_root")
        .unwrap();

    match &result.result {
        Some(LiteralValue::Number(n)) => assert_eq!(*n, Decimal::from_str("2").unwrap()),
        _ => panic!("Expected number, got {:?}", result.result),
    }
}

#[test]
fn test_combined_operations() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test
fact x = 10
fact y = 3
rule calculation = (x % y) + (2 ^ 3)
"#,
            "test",
        )
        .unwrap();

    let response = engine.evaluate("test", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "calculation")
        .unwrap();

    match &result.result {
        Some(LiteralValue::Number(n)) => assert_eq!(*n, Decimal::from_str("9").unwrap()),
        _ => panic!("Expected number, got {:?}", result.result),
    }
}
