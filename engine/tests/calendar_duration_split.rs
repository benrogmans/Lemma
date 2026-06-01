use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("test.lemma")))
}

fn eval_rule(code: &str, spec_name: &str, rule_name: &str) -> String {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let now = DateTimeValue::now();
    let response = engine
        .run(None, spec_name, Some(&now), HashMap::new(), false)
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .display
        .clone()
        .expect("display")
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
        combined.contains(expected_fragment),
        "Expected error containing '{}', got: {}",
        expected_fragment,
        combined
    );
}

#[test]
fn calendar_add_calendar_and_convert_to_months() {
    let code = r#"spec s
uses lemma units
data a: 1 year
data b: 6 month
rule total: (a + b) as month"#;
    let val = eval_rule(code, "s", "total");
    assert!(
        val.contains("18") && val.to_lowercase().contains("month"),
        "Expected 18 month, got: {val}"
    );
}

#[test]
fn date_add_calendar_uses_exact_month_arithmetic() {
    let code = r#"spec s
uses lemma units
data start: 2024-01-31
rule end: start + 1 month"#;
    let val = eval_rule(code, "s", "end");
    assert!(
        val.contains("2024-02-29"),
        "Expected leap-year month addition, got: {val}"
    );
}

#[test]
fn duration_and_calendar_addition_rejected() {
    let code = r#"spec s
uses lemma units
data a: 1 weeks
data b: 1 month
rule total: a + b"#;
    expect_plan_error(code, "Cannot add unrelated quantity types");
}

#[test]
fn calendar_to_duration_conversion_rejected() {
    let code = r#"spec s
uses lemma units
data c: 1 month
rule seconds: c as seconds as number"#;
    expect_plan_error(code, "different quantity families");
}

#[test]
fn duration_typed_slot_rejects_calendar_default() {
    let code = r#"spec s
uses lemma units
data d: units.duration -> default 1 month"#;
    expect_plan_error(code, "Unit 'month' is for calendar data");
    expect_plan_error(code, "Valid 'duration' units are");
}

#[test]
fn weight_quantity_calendar_default_lists_quantity_units() {
    let code = r#"spec s
uses lemma units
data weight: quantity -> unit gram 1 -> unit kilogram 1000
data w: weight -> default 1 month"#;
    expect_plan_error(code, "Unit 'month' is for calendar data");
    expect_plan_error(code, "Valid 'weight' units are");
}

#[test]
fn text_calendar_default_suggests_quoted_text() {
    let code = r#"spec s
uses lemma units
data notes: text -> default 1 month"#;
    expect_plan_error(code, "Unit 'month' is for calendar data");
    expect_plan_error(code, "double quotes");
}
