use lemma::parsing::ast::{DateTimeValue, TimezoneValue};
use lemma::{Engine, LiteralValue, ValueKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("test.lemma")))
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

fn eval_literal_with_data(
    code: &str,
    spec_name: &str,
    rule_name: &str,
    data: HashMap<String, String>,
) -> LiteralValue {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let response = engine
        .run(
            None,
            spec_name,
            Some(&default_effective()),
            data,
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

fn eval_literal(code: &str, spec_name: &str, rule_name: &str) -> LiteralValue {
    eval_literal_with_data(code, spec_name, rule_name, HashMap::new())
}

fn eval_rule(code: &str, spec_name: &str, rule_name: &str) -> String {
    eval_literal(code, spec_name, rule_name).to_string()
}

fn eval_bool(code: &str, spec_name: &str, rule_name: &str) -> bool {
    match eval_literal(code, spec_name, rule_name).value {
        ValueKind::Boolean(val) => val,
        other => panic!("Expected Boolean, got {:?}", other),
    }
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

// =============================================================================
// P. Number range
// =============================================================================

#[test]
fn p1_containment_inside() {
    let code = r#"spec test
rule ok: 50 in 0...100"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p2_containment_outside() {
    let code = r#"spec test
rule ok: 150 in 0...100"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn p3_containment_at_boundary_left() {
    let code = r#"spec test
rule ok: 0 in 0...100"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p4_containment_at_boundary_right_excluded() {
    let code = r#"spec test
rule ok: 100 in 0...100"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn p4b_containment_just_inside_right_boundary() {
    let code = r#"spec test
rule ok: 99 in 0...100"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p4c_empty_range_contains_nothing() {
    let code = r#"spec test
rule ok: 5 in 5...5"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn p5_containment_negative_range() {
    let code = r#"spec test
rule ok: -25 in -50...50"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p6_containment_fully_negative() {
    let code = r#"spec test
rule ok: -75 in -100...-50"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p7_containment_just_outside_left() {
    let code = r#"spec test
rule ok: -1 in 0...100"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn p8_span_gte() {
    let code = r#"spec test
rule ok: (0...100) >= 50"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p9_span_lt() {
    let code = r#"spec test
rule ok: (0...100) < 50"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn p10_span_negative_range() {
    let code = r#"spec test
rule ok: (-100...-50) >= 49"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p11_span_empty_range() {
    let code = r#"spec test
rule ok: (50...50) >= 1"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn p12_span_reversed_range_is_absolute() {
    let code = r#"spec test
rule ok: (100...0) >= 50"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p13_addition_uses_range_size() {
    let code = r#"spec test
rule value: (0...100) + 50"#;
    assert_eq!(eval_rule(code, "test", "value"), "150");
}

#[test]
fn p14_subtraction_uses_range_size() {
    let code = r#"spec test
rule value: (0...100) - 50"#;
    assert_eq!(eval_rule(code, "test", "value"), "50");
}

#[test]
fn p15_chained_arithmetic_uses_range_size() {
    let code = r#"spec test
rule base: 0...100
rule adjusted: base + 50 - 10"#;
    assert_eq!(eval_rule(code, "test", "adjusted"), "140");
}

#[test]
fn p16_negative_addition_uses_range_size() {
    let code = r#"spec test
rule value: (0...100) + -20"#;
    assert_eq!(eval_rule(code, "test", "value"), "80");
}

#[test]
fn p17_comparison_after_range_arithmetic() {
    let code = r#"spec test
rule ok: ((0...100) + 50) >= 149"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn p18_range_through_rules() {
    let code = r#"spec test
data lower: number -> default 0
data upper: number -> default 100
data value: number -> default 50
rule bounds: lower...upper
rule check: value in bounds"#;
    assert!(eval_bool(code, "test", "check"));
}

#[test]
fn p19_adjusted_range_through_rules() {
    let code = r#"spec test
data lower: 0
data upper: 100
data adjustment: 25
rule bounds: lower...upper
rule adjusted: bounds + adjustment"#;
    assert_eq!(eval_rule(code, "test", "adjusted"), "125");
}

#[test]
fn p20_user_declared() {
    let code = r#"spec test
data bounds: number range -> default 0...100
rule check: 50 in bounds"#;
    assert!(eval_bool(code, "test", "check"));
}

#[test]
fn p21_user_declared_dynamic() {
    let code = r#"spec test
data bounds: number range
rule check: 50 in bounds"#;
    let mut data = HashMap::new();
    data.insert("bounds".to_string(), "0...100".to_string());
    let lit = eval_literal_with_data(code, "test", "check", data);
    match lit.value {
        ValueKind::Boolean(val) => assert!(val),
        other => panic!("Expected Boolean, got {:?}", other),
    }
}

// =============================================================================
// Q. Quantity range
// =============================================================================

#[test]
fn q1_containment_same_unit() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: 32 kilogram in 30 kilogram...35 kilogram"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn q2_containment_outside() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: 36 kilogram in 30 kilogram...35 kilogram"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn q3_containment_cross_unit() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: 32000 gram in 30 kilogram...35 kilogram"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn q4_containment_cross_unit_outside() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: 29000 gram in 30 kilogram...35 kilogram"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn q5_containment_at_boundary_cross_unit() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: 30000 gram in 30 kilogram...35 kilogram"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn q5b_containment_at_right_boundary_excluded() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: 35 kilogram in 30 kilogram...35 kilogram"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn q5c_containment_just_inside_right_boundary() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: 34 kilogram in 30 kilogram...35 kilogram"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn q6_span_gte_same_unit() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: (30 kilogram...35 kilogram) >= 5 kilogram"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn q7_span_gte_cross_unit() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: (30 kilogram...35 kilogram) >= 5000 gram"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn q8_span_lt_cross_unit() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: (30 kilogram...35 kilogram) >= 6000 gram"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn q9_span_mixed_range_units() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: (1000 gram...5 kilogram) >= 4 kilogram"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn q12_subtraction_cross_unit_uses_range_size() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule lower_bound: ((30 kilogram...35 kilogram) - 500 gram) >= 4500 gram
rule upper_bound: ((30 kilogram...35 kilogram) - 500 gram) > 4500 gram"#;
    assert!(eval_bool(code, "test", "lower_bound"));
    assert!(!eval_bool(code, "test", "upper_bound"));
}

#[test]
fn q13_comparison_after_range_arithmetic() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule ok: ((30 kilogram...35 kilogram) + 2000 gram) >= 6500 gram"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn q14_range_through_rules() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
data min_weight: 30 kilogram
data max_weight: 35 kilogram
data sample: 32 kilogram
rule acceptable: min_weight...max_weight
rule span_with_allowance: acceptable + 500 gram
rule passes: sample in acceptable
rule margin_ok: span_with_allowance >= 5500 gram"#;
    assert!(eval_bool(code, "test", "passes"));
    assert!(eval_bool(code, "test", "margin_ok"));
}

#[test]
fn q15_user_declared() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
data acceptable: quantity range -> default 30 kilogram...35 kilogram
rule check: 32 kilogram in acceptable"#;
    assert!(eval_bool(code, "test", "check"));
}

#[test]
fn q15_mixed_unit_endpoints_gram_to_kilogram() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
data band: quantity range -> default 3000 gram...35 kilogram
rule inside: 3200 gram in band
rule lower: 3000 gram in band
rule upper_excluded: 35 kilogram in band"#;
    assert!(eval_bool(code, "test", "inside"));
    assert!(eval_bool(code, "test", "lower"));
    assert!(!eval_bool(code, "test", "upper_excluded"));
}

// =============================================================================
// R. Ratio range
// =============================================================================

#[test]
fn r1_containment_inside() {
    let code = r#"spec test
rule ok: 25% in 10%...50%"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn r2_containment_outside() {
    let code = r#"spec test
rule ok: 75% in 10%...50%"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn r3_containment_at_boundary_right_excluded() {
    let code = r#"spec test
rule ok: 50% in 10%...50%"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn r3b_containment_just_inside_right_boundary() {
    let code = r#"spec test
rule ok: 49% in 10%...50%"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn r4_containment_zero() {
    let code = r#"spec test
rule ok: 0% in 0%...100%"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn r5_span_gte() {
    let code = r#"spec test
rule ok: (10%...50%) >= 30%"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn r6_span_lt() {
    let code = r#"spec test
rule ok: (10%...50%) >= 50%"#;
    assert!(!eval_bool(code, "test", "ok"));
}

#[test]
fn r7_addition_uses_range_size() {
    let code = r#"spec test
rule value: (10%...50%) + 10%"#;
    assert_eq!(eval_rule(code, "test", "value"), "50%");
}

#[test]
fn r8_subtraction_uses_range_size() {
    let code = r#"spec test
rule value: (10%...50%) - 10%"#;
    assert_eq!(eval_rule(code, "test", "value"), "30%");
}

#[test]
fn r9_comparison_after_range_arithmetic() {
    let code = r#"spec test
rule ok: ((10%...50%) + 10%) >= 45%"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn r10_range_through_rules() {
    let code = r#"spec test
data base_discount: 10%
data max_discount: 50%
data customer_discount: 35%
rule discount_range: base_discount...max_discount
rule qualifies: customer_discount in discount_range
rule generous_enough: discount_range >= 30%"#;
    assert!(eval_bool(code, "test", "qualifies"));
    assert!(eval_bool(code, "test", "generous_enough"));
}

// =============================================================================
// S. Range validation / rejection
// =============================================================================

#[test]
fn s1_date_vs_number() {
    let code = r#"spec test
rule bad: 2024-01-01...100"#;
    expect_plan_error(code, "range");
}

#[test]
fn s2_quantity_family_mismatch() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
data money: quantity -> unit eur 1.00
rule bad: 30 kilogram...100 eur"#;
    expect_plan_error(code, "range");
}

#[test]
fn s3_boolean_range() {
    let code = r#"spec test
rule bad: true...false"#;
    expect_plan_error(code, "range");
}

#[test]
fn s4_text_range() {
    let code = r#"spec test
rule bad: "a"..."z""#;
    expect_plan_error(code, "range");
}

#[test]
fn s5_date_vs_duration() {
    let code = r#"spec test
uses lemma si
rule bad: 2024-01-01...7 days"#;
    expect_plan_error(code, "range");
}

#[test]
fn s6_number_vs_ratio() {
    let code = r#"spec test
rule bad: 50...50%"#;
    expect_plan_error(code, "range");
}

// `(0...100) as days` span semantics: see `range_span_unit_conversion.rs`.

#[test]
fn s8_quantity_range_in_months() {
    let code = r#"spec test
data weight: quantity -> unit gram 1 -> unit kilogram 1000
rule bad: (30 kilogram...35 kilogram) as months"#;
    expect_plan_error(code, "convert");
}

#[test]
fn s9_quantity_times_number_range() {
    let code = r#"spec test
uses lemma si
data money: quantity -> unit eur 1.00
data rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_hour eur/hour
data hourly_rate: 50 eur_per_hour
rule bad: hourly_rate * (0...100)"#;
    expect_plan_error(code, "range");
}
