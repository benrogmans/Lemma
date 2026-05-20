use lemma::parsing::ast::{DateTimeValue, TimezoneValue};
use lemma::{Engine, LiteralValue, ValueKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("calendar_range.lemma")))
}

fn default_effective() -> DateTimeValue {
    DateTimeValue {
        year: 2026,
        month: 3,
        day: 7,
        hour: 12,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: 0,
            offset_minutes: 0,
        }),
    }
}

fn eval_bool(code: &str, spec_name: &str, rule_name: &str) -> bool {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let response = engine
        .run(
            None,
            spec_name,
            Some(&default_effective()),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("Should evaluate");
    match &response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .result
        .value()
        .unwrap_or_else(|| panic!("Rule '{}' returned non-value", rule_name))
        .value
    {
        ValueKind::Boolean(v) => *v,
        other => panic!("Expected boolean, got {:?}", other),
    }
}

fn eval_literal(code: &str, spec_name: &str, rule_name: &str) -> LiteralValue {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let response = engine
        .run(
            None,
            spec_name,
            Some(&default_effective()),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .result
        .value()
        .unwrap_or_else(|| panic!("Rule '{}' returned non-value", rule_name))
        .clone()
}

fn expect_plan_error(code: &str, expected_fragment: &str) {
    let mut engine = Engine::new();
    let result = engine.load(code, source());
    assert!(result.is_err(), "Expected planning error");
    let combined = result
        .unwrap_err()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        combined
            .to_lowercase()
            .contains(&expected_fragment.to_lowercase()),
        "Expected error containing {:?}, got: {}",
        expected_fragment,
        combined
    );
}

#[test]
fn containment_inside_band() {
    let code = r#"spec age_band
data age: 25 years
rule ok: age in 18 years...67 years"#;
    assert!(eval_bool(code, "age_band", "ok"));
}

#[test]
fn containment_half_open_upper() {
    let code = r#"spec age_band
data age: 67 years
rule ok: age in 18 years...67 years"#;
    assert!(!eval_bool(code, "age_band", "ok"));
}

#[test]
fn containment_mixed_calendar_units() {
    let code = r#"spec mixed
data age: 18 months
rule ok: age in 1 year...2 years"#;
    assert!(eval_bool(code, "mixed", "ok"));
}

#[test]
fn data_default_calendar_range() {
    let code = r#"spec band
data band: calendar range -> default 18 years...67 years
rule span: (band + 0 years) >= 5 years"#;
    assert!(eval_bool(code, "band", "span"));
}

#[test]
fn range_plus_calendar_shifts_upper() {
    let code = r#"spec shift
rule upper: (18 years...67 years) + 2 years"#;
    let value = eval_literal(code, "shift", "upper");
    match &value.value {
        ValueKind::Range(left, right) => {
            assert_eq!(left.to_string(), "18 years");
            assert_eq!(right.to_string(), "69 years");
        }
        other => panic!("Expected range, got {:?}", other),
    }
}

#[test]
fn compare_span_against_calendar_scalar() {
    let code = r#"spec compare
rule ok: (18 years...67 years) >= 5 years"#;
    assert!(eval_bool(code, "compare", "ok"));
}

#[test]
fn bare_number_range_unchanged() {
    let code = r#"spec nums
rule r: 15 in 12...18"#;
    assert!(eval_bool(code, "nums", "r"));
}

#[test]
fn reject_mixed_calendar_and_duration_units() {
    let code = r#"spec bad
rule bad: 12 years...7 days"#;
    expect_plan_error(code, "range");
}

#[test]
fn reject_date_and_calendar_endpoints() {
    let code = r#"spec bad
rule bad: 2024-01-01...18 years"#;
    expect_plan_error(code, "range");
}
