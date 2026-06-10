use lemma::DateTimeValue;
use lemma::*;
use std::collections::HashMap;

fn assert_rule_number(rule: &lemma::RuleResult, expected: rust_decimal::Decimal) {
    assert!(!rule.vetoed, "unexpected veto: {:?}", rule.veto_reason);
    let lit = rule
        .explanation
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    if let lemma::ValueKind::Number(n) = &lit.value {
        assert_eq!(
            lemma::ValueKind::Number(n.clone())
                .as_decimal_magnitude()
                .unwrap(),
            expected
        );
    } else {
        panic!("Expected number, got {:?}", lit.value);
    }
}

#[test]
fn test_modulo_simple() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test
data a: 10
data b: 3
rule remainder: a % b
"#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let result = response.results.get("remainder").unwrap();
    assert_rule_number(result, rust_decimal::Decimal::ONE);
}

#[test]
fn test_power_simple() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test
data base: 2
data exponent: 3
rule result: base ^ exponent
"#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let result = response.results.get("result").unwrap();
    assert_rule_number(result, rust_decimal::Decimal::from(8));
}

#[test]
fn test_modulo_in_expression() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test
data value: 17
rule is_even: (value % 2) is 0
rule is_odd: (value % 2) is 1
"#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test", Some(&now), HashMap::new(), true, None)
        .unwrap();

    let is_even = response.results.get("is_even").unwrap();
    assert_eq!(is_even.boolean, Some(false));

    let is_odd = response.results.get("is_odd").unwrap();
    assert_eq!(is_odd.boolean, Some(true));
}

#[test]
fn test_power_with_fractions() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test
data base: 4
rule square_root: base ^ 0.5
"#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let result = response.results.get("square_root").unwrap();
    assert_rule_number(result, rust_decimal::Decimal::from(2));
}

#[test]
fn test_combined_operations() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test
data x: 10
data y: 3
rule calculation: (x % y) + (2 ^ 3)
"#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let result = response.results.get("calculation").unwrap();
    assert_rule_number(result, rust_decimal::Decimal::from(9));
}
