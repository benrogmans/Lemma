use lemma::DateTimeValue;
use lemma::{ValueKind, *};
use std::collections::HashMap;

#[test]
fn test_percentage_subtract_rejected() {
    let code = r#"
spec pricing
data discount: 25%
rule net_multiplier: 1 - discount
"#;

    let mut engine = Engine::new();
    let result = engine.load(code, lemma::SourceType::Volatile);
    assert!(
        result.is_err(),
        "number - ratio should be rejected at planning"
    );
    let msg = result
        .unwrap_err()
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        msg.contains("scale explicitly"),
        "expected hint, got: {}",
        msg
    );
}

#[test]
fn test_duration_operations() {
    let code = r#"
spec scheduling
uses lemma units
data meeting_length: 30 minutes
rule double_meeting: meeting_length * 2
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "scheduling", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let rule = response.results.get("double_meeting").unwrap();
    let measure = rule.measure.as_ref().expect("measure map");
    assert_eq!(
        measure.get("minutes").map(String::as_str),
        Some("60"),
        "30 minutes * 2 = 60 minutes"
    );
}

#[test]
fn test_date_arithmetic_with_duration() {
    let code = r#"
spec dates
uses lemma units
data start: 2024-01-15
rule end: start + 7 days
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "dates", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let result = response
        .results
        .get("end")
        .unwrap()
        .explanation
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");

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
        .run(None, "logic", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let result = response
        .results
        .get("can_access")
        .unwrap()
        .explanation
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");

    match result {
        LiteralValue {
            value: ValueKind::Boolean(b),
            ..
        } => {
            assert!(b);
        }
        _ => panic!("Expected Boolean, got {:?}", result),
    }
}
