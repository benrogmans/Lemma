use lemma::{DateGranularity, DateTimeValue, Engine, LiteralValue, TimezoneValue, ValueKind};
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
        granularity: DateGranularity::DateTime,
    }
}

fn eval_bool(code: &str, spec_name: &str, rule_name: &str) -> bool {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let effective = default_effective();
    let plan = engine
        .get_plan(None, spec_name, Some(&effective))
        .expect("plan");
    let response = engine
        .run_plan(
            plan,
            Some(&effective),
            HashMap::new(),
            true,
            Some(&[rule_name.to_string()]),
        )
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .boolean
        .expect("boolean rule result")
}

fn eval_literal(code: &str, spec_name: &str, rule_name: &str) -> LiteralValue {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let effective = default_effective();
    let plan = engine
        .get_plan(None, spec_name, Some(&effective))
        .expect("plan");
    let response = engine
        .run_plan(
            plan,
            Some(&effective),
            HashMap::new(),
            true,
            Some(&[rule_name.to_string()]),
        )
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .explanation
        .as_ref()
        .expect("explanation")
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
uses lemma units
data age: 25 year
rule ok: age in 18 year...67 year"#;
    assert!(eval_bool(code, "age_band", "ok"));
}

#[test]
fn containment_half_open_upper() {
    let code = r#"spec age_band
uses lemma units
data age: 67 year
rule ok: age in 18 year...67 year"#;
    assert!(!eval_bool(code, "age_band", "ok"));
}

#[test]
fn containment_mixed_calendar_units() {
    let code = r#"spec mixed
uses lemma units
data age: 18 month
rule ok: age in 1 year...2 year"#;
    assert!(eval_bool(code, "mixed", "ok"));
}

#[test]
fn data_default_calendar_range() {
    let code = r#"spec band
uses lemma units
data band: units.calendar -> default 18 year...67 year
rule span: 30 year in band"#;
    assert!(eval_bool(code, "band", "span"));
}

#[test]
fn range_plus_calendar_shifts_upper() {
    let code = r#"spec shift
uses lemma units
rule upper: (18 year...67 year) + 2 year"#;
    let value = eval_literal(code, "shift", "upper");
    match &value.value {
        ValueKind::Range(left, right) => {
            assert_eq!(left.to_string(), "18 year");
            assert_eq!(right.to_string(), "69 year");
        }
        other => panic!("Expected range, got {:?}", other),
    }
}

#[test]
fn compare_span_against_calendar_scalar() {
    let code = r#"spec compare
uses lemma units
rule ok: (18 year...67 year) >= 5 year"#;
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
uses lemma units
rule bad: 12 year...7 days"#;
    expect_plan_error(code, "range");
}

#[test]
fn reject_date_and_calendar_endpoints() {
    let code = r#"spec bad
uses lemma units
rule bad: 2024-01-01...18 year"#;
    expect_plan_error(code, "range");
}
