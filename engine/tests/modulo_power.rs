use lemma::parsing::ast::DateTimeValue;
use lemma::*;
use std::collections::HashMap;
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
        .run(
            None,
            "test",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let result = response.results.get("remainder").unwrap();

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(n) = &lit.value {
                assert_eq!(
                    lemma::commit_rational_to_decimal(n).unwrap(),
                    rust_decimal::Decimal::ONE
                )
            } else {
                panic!("Expected number, got {:?}", result.result);
            }
        }
        _ => panic!("Expected number, got {:?}", result.result),
    }
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
        .run(
            None,
            "test",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let result = response.results.get("result").unwrap();

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(n) = &lit.value {
                assert_eq!(
                    lemma::commit_rational_to_decimal(n).unwrap(),
                    rust_decimal::Decimal::from(8)
                )
            } else {
                panic!("Expected number, got {:?}", result.result);
            }
        }
        _ => panic!("Expected number, got {:?}", result.result),
    }
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
        .run(
            None,
            "test",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let is_even = response.results.get("is_even").unwrap();
    assert_eq!(
        is_even.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(false)))
    );

    let is_odd = response.results.get("is_odd").unwrap();
    assert_eq!(
        is_odd.result,
        lemma::OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );
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
        .run(
            None,
            "test",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let result = response.results.get("square_root").unwrap();

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(n) = &lit.value {
                assert_eq!(
                    lemma::commit_rational_to_decimal(n).unwrap(),
                    rust_decimal::Decimal::from(2)
                );
            } else {
                panic!("Expected number result");
            }
        }
        _ => panic!("Expected number, got {:?}", result.result),
    }
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
        .run(
            None,
            "test",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let result = response.results.get("calculation").unwrap();

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(n) = &lit.value {
                assert_eq!(
                    lemma::commit_rational_to_decimal(n).unwrap(),
                    rust_decimal::Decimal::from(9)
                );
            } else {
                panic!("Expected number, got {:?}", result.result);
            }
        }
        _ => panic!("Expected number, got {:?}", result.result),
    }
}
