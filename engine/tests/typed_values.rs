use lemma::parsing::ast::DateTimeValue;
use lemma::{ValueKind, *};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn test_percentage_arithmetic() {
    let code = r#"
spec pricing
data discount: 25%
rule net_multiplier: 1 - discount
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
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
        } => assert_eq!(
            lemma::commit_rational_to_decimal(n).unwrap(),
            Decimal::new(75, 2)
        ),
        _ => panic!("Expected Number, got {:?}", result),
    }
}

#[test]
fn test_duration_operations() {
    let code = r#"
spec scheduling
uses lemma si
data meeting_length: 30 minutes
rule double_meeting: meeting_length * 2
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "scheduling",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
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
            value: ValueKind::Quantity(value, unit, _),
            ..
        } => {
            // 30 minutes * 2 = 60 (stored as the numeric value in minutes unit)
            assert_eq!(
                lemma::commit_rational_to_decimal(value).unwrap(),
                Decimal::from(60)
            );
            assert!(
                unit.to_ascii_lowercase().contains("minute"),
                "unit={unit:?}"
            );
        }
        _ => panic!("Expected Quantity, got {:?}", result),
    }
}

#[test]
fn test_date_arithmetic_with_duration() {
    let code = r#"
spec dates
uses lemma si
data start: 2024-01-15
rule end: start + 7 days
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "dates",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
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
spec logic
data is_active: true
data is_premium: false
rule can_access: is_active and not is_premium
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "logic",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
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
