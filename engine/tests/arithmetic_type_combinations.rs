//! Comprehensive tests for arithmetic type combinations.
//!
//! Tests every allowed and disallowed combination of types across all
//! arithmetic operators (+, -, *, /, %, ^), verifying both that valid
//! combinations produce the correct result type and that invalid
//! combinations are rejected during validation.

use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

fn with_duration_typedef(code: &str) -> String {
    code.replacen("spec t\n", "spec t\nuses lemma si\n", 1)
}

fn eval_rule(
    code: &str,
    spec_name: &str,
    rule_name: &str,
    data: HashMap<String, String>,
) -> String {
    let code = with_duration_typedef(code);
    let mut engine = Engine::new();
    engine
        .load(&code, lemma::SourceType::Volatile)
        .expect("Should parse and plan");
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            spec_name,
            Some(&now),
            data,
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("Should evaluate");
    let result = response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' should exist", rule_name));
    result
        .result
        .value()
        .unwrap_or_else(|| {
            panic!(
                "Rule '{}' should have a value, got: {:?}",
                rule_name, result.result
            )
        })
        .to_string()
}

fn expect_plan_error(code: &str, expected_fragment: &str) {
    let code = with_duration_typedef(code);
    let mut engine = Engine::new();
    let result = engine.load(&code, lemma::SourceType::Volatile);
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

// ═══════════════════════════════════════════════════════════════════
// Number with Number
// ═══════════════════════════════════════════════════════════════════

#[test]
fn number_add_number() {
    let code = r#"spec t
data a: 10
data b: 3
rule result: a + b"#;
    assert_eq!(eval_rule(code, "t", "result", HashMap::new()), "13");
}

#[test]
fn number_subtract_number() {
    let code = r#"spec t
data a: 10
data b: 3
rule result: a - b"#;
    assert_eq!(eval_rule(code, "t", "result", HashMap::new()), "7");
}

#[test]
fn number_multiply_number() {
    let code = r#"spec t
data a: 10
data b: 3
rule result: a * b"#;
    assert_eq!(eval_rule(code, "t", "result", HashMap::new()), "30");
}

#[test]
fn number_divide_number() {
    let code = r#"spec t
data a: 12
data b: 4
rule result: a / b"#;
    assert_eq!(eval_rule(code, "t", "result", HashMap::new()), "3");
}

#[test]
fn number_modulo_number() {
    let code = r#"spec t
data a: 10
data b: 3
rule result: a % b"#;
    assert_eq!(eval_rule(code, "t", "result", HashMap::new()), "1");
}

#[test]
fn number_power_number() {
    let code = r#"spec t
data a: 2
data b: 3
rule result: a ^ b"#;
    assert_eq!(eval_rule(code, "t", "result", HashMap::new()), "8");
}

// ═══════════════════════════════════════════════════════════════════
// Quantity with Number → Quantity
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_add_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 10 eur
data n: 5
rule result: price + n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("15"), "Expected 15 eur, got: {}", val);
}

#[test]
fn quantity_subtract_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 10 eur
data n: 3
rule result: price - n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("7"), "Expected 7 eur, got: {}", val);
}

#[test]
fn quantity_multiply_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 10 eur
data n: 3
rule result: price * n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("30"), "Expected 30 eur, got: {}", val);
}

#[test]
fn number_multiply_quantity() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data n: 3
data price: 10 eur
rule result: n * price"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("30"), "Expected 30 eur, got: {}", val);
}

#[test]
fn quantity_divide_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 12 eur
data n: 4
rule result: price / n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("3"), "Expected 3 eur, got: {}", val);
}

#[test]
fn quantity_modulo_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 10 eur
data n: 3
rule result: price % n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("1"), "Expected 1 eur, got: {}", val);
}

#[test]
fn quantity_power_number() {
    // Exponent must be an integer literal for dimensional types; using a literal 3 directly.
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 2 eur
rule result: price ^ 3"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("8"), "Expected 8 eur, got: {}", val);
}

#[test]
fn quantity_power_variable_exponent_rejected() {
    // Variable exponent for Quantity ^ Number must be rejected at plan time.
    let mut engine = Engine::new();
    let result = engine.load(
        r#"spec t
data money: quantity -> unit eur 1.00
data price: 2 eur
data n: 3
rule result: price ^ n"#,
        lemma::SourceType::Volatile,
    );
    assert!(
        result.is_err(),
        "Quantity ^ variable_exponent should be rejected at plan time"
    );
}

#[test]
fn quantity_power_fractional_exponent_rejected() {
    // Fractional literal exponents for Quantity ^ Number must also be rejected.
    let mut engine = Engine::new();
    let result = engine.load(
        r#"spec t
data money: quantity -> unit eur 1.00
data price: 4 eur
rule result: price ^ 0.5"#,
        lemma::SourceType::Volatile,
    );
    assert!(
        result.is_err(),
        "Quantity ^ fractional_exponent should be rejected at plan time"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Quantity with Ratio → Quantity
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_add_ratio() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 100 eur
data rate: 10%
rule result: price + rate"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("110"), "Expected 110 eur, got: {}", val);
}

#[test]
fn quantity_subtract_ratio() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 100 eur
data discount: 25%
rule result: price - discount"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("75"), "Expected 75 eur, got: {}", val);
}

#[test]
fn quantity_multiply_ratio() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 100 eur
data rate: 50%
rule result: price * rate"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("50"), "Expected 50 eur, got: {}", val);
}

#[test]
fn quantity_divide_ratio() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data price: 100 eur
data rate: 50%
rule result: price / rate"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("200"), "Expected 200 eur, got: {}", val);
}

// ═══════════════════════════════════════════════════════════════════
// Quantity with Duration → anonymous intermediate (canonical magnitudes).
// Phase 1 dimensional arithmetic: operands are converted to canonical magnitudes before
// the operation. eur has factor 1 (canonical), hours converts to 3600 seconds.
// 50 eur * 8 hours → 50 * 28800 seconds = 1 440 000 (anonymous {money:1, duration:1}).
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_multiply_duration_rejected_at_rule_boundary() {
    // Duration * Quantity and Quantity * Duration produce anonymous intermediates with unresolved
    // dimensions. These are forbidden at rule boundaries; the user must cast with `as <unit>`.
    let mut engine = Engine::new();
    let result = engine.load(
        r#"spec t
data money: quantity -> unit eur 1.00
data rate: 50 eur
data hours: 8 hours
rule result: rate * hours"#,
        lemma::SourceType::Volatile,
    );
    assert!(
        result.is_err(),
        "Quantity * Duration at rule boundary should be rejected: anonymous intermediate {{money:1, duration:1}}"
    );
}

#[test]
fn duration_multiply_quantity_rejected_at_rule_boundary() {
    let mut engine = Engine::new();
    let result = engine.load(
        r#"spec t
data money: quantity -> unit eur 1.00
data hours: 8 hours
data rate: 50 eur
rule result: hours * rate"#,
        lemma::SourceType::Volatile,
    );
    assert!(
        result.is_err(),
        "Duration * Quantity at rule boundary should be rejected: anonymous intermediate {{duration:1, money:1}}"
    );
}

#[test]
fn quantity_divide_duration_rejected_at_rule_boundary() {
    let mut engine = Engine::new();
    let result = engine.load(
        r#"spec t
data money: quantity -> unit eur 1.00
data total: 400 eur
data hours: 8 hours
rule result: total / hours"#,
        lemma::SourceType::Volatile,
    );
    assert!(
        result.is_err(),
        "Quantity / Duration at rule boundary should be rejected: anonymous intermediate {{money:1, duration:-1}}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Duration with Number → Duration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn duration_add_number() {
    let code = r#"spec t
data d: 10 hours
data n: 5
rule result: d + n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("15"), "Expected 15 hours, got: {}", val);
}

#[test]
fn duration_subtract_number() {
    let code = r#"spec t
data d: 10 hours
data n: 3
rule result: d - n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("7"), "Expected 7 hours, got: {}", val);
}

#[test]
fn duration_multiply_number() {
    let code = r#"spec t
data d: 10 hours
data n: 3
rule result: d * n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("30"), "Expected 30 hours, got: {}", val);
}

#[test]
fn number_multiply_duration() {
    let code = r#"spec t
data n: 3
data d: 10 hours
rule result: n * d"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("30"), "Expected 30 hours, got: {}", val);
}

#[test]
fn duration_divide_number() {
    let code = r#"spec t
data d: 12 hours
data n: 4
rule result: d / n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("3"), "Expected 3 hours, got: {}", val);
}

#[test]
fn duration_modulo_number() {
    let code = r#"spec t
data d: 10 hours
data n: 3
rule result: d % n"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("1"), "Expected 1 hour, got: {}", val);
}

#[test]
fn duration_power_number() {
    // Exponent must be an integer literal for dimensional types; using a literal 3 directly.
    let code = r#"spec t
data d: 2 hours
rule result: d ^ 3"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("8"), "Expected 8 hours, got: {}", val);
}

#[test]
fn duration_power_variable_exponent_rejected() {
    // Variable exponent for Duration ^ Number must be rejected at plan time.
    let mut engine = Engine::new();
    let result = engine.load(
        r#"spec t
data d: 2 hours
data n: 3
rule result: d ^ n"#,
        lemma::SourceType::Volatile,
    );
    assert!(
        result.is_err(),
        "Duration ^ variable_exponent should be rejected at plan time"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Duration with Ratio → Duration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn duration_add_ratio() {
    let code = r#"spec t
data d: 10 hours
data r: 50%
rule result: d + r"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("15"), "Expected 15 hours, got: {}", val);
}

#[test]
fn duration_subtract_ratio() {
    let code = r#"spec t
data d: 10 hours
data r: 25%
rule result: d - r"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("7.5"), "Expected 7.5 hours, got: {}", val);
}

#[test]
fn duration_multiply_ratio() {
    let code = r#"spec t
data d: 10 hours
data r: 50%
rule result: d * r"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("5"), "Expected 5 hours, got: {}", val);
}

#[test]
fn ratio_multiply_duration() {
    let code = r#"spec t
data r: 50%
data d: 10 hours
rule result: r * d"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("5"), "Expected 5 hours, got: {}", val);
}

#[test]
fn duration_divide_ratio() {
    let code = r#"spec t
data d: 10 hours
data r: 50%
rule result: d / r"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("20"), "Expected 20 hours, got: {}", val);
}

// ═══════════════════════════════════════════════════════════════════
// Ratio with Number → Number
// ═══════════════════════════════════════════════════════════════════

#[test]
fn ratio_multiply_number() {
    let code = r#"spec t
data r: 50%
data n: 200
rule result: r * n"#;
    assert_eq!(eval_rule(code, "t", "result", HashMap::new()), "100");
}

#[test]
fn ratio_add_number() {
    let code = r#"spec t
data r: 10%
data n: 100
rule result: n + r"#;
    assert_eq!(eval_rule(code, "t", "result", HashMap::new()), "110");
}

// ═══════════════════════════════════════════════════════════════════
// Quantity with Quantity (same family) → Quantity
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_add_quantity_same_family() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 4 eur
data b: 5 eur
rule result: a + b"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(
        val.contains("9") && val.contains("eur"),
        "Expected 9 eur, got: {}",
        val
    );
}

#[test]
fn quantity_subtract_quantity_same_family() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 3 eur
rule result: a - b"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(
        val.contains("7") && val.contains("eur"),
        "Expected 7 eur, got: {}",
        val
    );
}

#[test]
fn quantity_add_quantity_result_used_in_comparison() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 4 eur
data b: 5 eur
data threshold: 8 eur
rule total: a + b
rule over_threshold: total > threshold"#;
    assert_eq!(
        eval_rule(code, "t", "over_threshold", HashMap::new()),
        "true"
    );
}

#[test]
fn quantity_add_quantity_result_in_further_arithmetic() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 20 eur
data c: 5 eur
rule subtotal: a + b
rule total: subtotal + c"#;
    let val = eval_rule(code, "t", "total", HashMap::new());
    assert!(
        val.contains("35") && val.contains("eur"),
        "Expected 35 eur, got: {}",
        val
    );
}

// ═══════════════════════════════════════════════════════════════════
// Ratio with Ratio → Ratio
// ═══════════════════════════════════════════════════════════════════

#[test]
fn ratio_add_ratio() {
    let code = r#"spec t
data a: 10%
data b: 5%
rule result: a + b"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("15"), "Expected 15 percent, got: {}", val);
}

#[test]
fn ratio_subtract_ratio() {
    let code = r#"spec t
data a: 25%
data b: 10%
rule result: a - b"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("15"), "Expected 15 percent, got: {}", val);
}

#[test]
fn ratio_add_ratio_result_used_with_quantity() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data base_rate: 10%
data surcharge: 5%
data price: 200 eur
rule combined_rate: base_rate + surcharge
rule discount: price * combined_rate"#;
    let val = eval_rule(code, "t", "discount", HashMap::new());
    assert!(
        val.contains("30"),
        "Expected 30 eur (200 * 15%), got: {}",
        val
    );
}

// ═══════════════════════════════════════════════════════════════════
// Date - Date rejection
// ═══════════════════════════════════════════════════════════════════

#[test]
fn date_subtract_date_result_used_in_comparison_with_duration() {
    let code = r#"spec t
data start: 2024-01-01
data end: 2024-01-10
data limit: 5 days
rule elapsed: end - start
rule over_limit: elapsed > limit"#;
    expect_plan_error(code, "Cannot subtract dates");
}

// ═══════════════════════════════════════════════════════════════════
// Duration with Duration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn duration_add_duration() {
    let code = r#"spec t
data a: 10 hours
data b: 5 hours
rule result: a + b"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("15"), "Expected 15 hours, got: {}", val);
}

#[test]
fn duration_subtract_duration() {
    let code = r#"spec t
data a: 10 hours
data b: 3 hours
rule result: a - b"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(val.contains("7"), "Expected 7 hours, got: {}", val);
}

// ═══════════════════════════════════════════════════════════════════
// Date/Time temporal arithmetic
// ═══════════════════════════════════════════════════════════════════

#[test]
fn date_add_duration() {
    let code = r#"spec t
data d: 2024-01-01
data dur: 7 days
rule result: d + dur"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(
        val.contains("2024-01-08"),
        "Expected 2024-01-08, got: {}",
        val
    );
}

#[test]
fn date_subtract_duration() {
    let code = r#"spec t
data d: 2024-01-08
data dur: 7 days
rule result: d - dur"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(
        val.contains("2024-01-01"),
        "Expected 2024-01-01, got: {}",
        val
    );
}

#[test]
fn duration_add_date() {
    let code = r#"spec t
data dur: 7 days
data d: 2024-01-01
rule result: dur + d"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(
        val.contains("2024-01-08"),
        "Expected 2024-01-08, got: {}",
        val
    );
}

#[test]
fn date_subtract_date() {
    let code = r#"spec t
data a: 2024-01-10
data b: 2024-01-01
rule result: a - b"#;
    expect_plan_error(code, "dateA...dateB");
}

// ═══════════════════════════════════════════════════════════════════
// Quantity family: parent + child (same family) → Quantity
// ═══════════════════════════════════════════════════════════════════

#[test]
fn same_family_parent_plus_child() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data budget: money -> unit jpy 160.00 -> minimum 0 eur
data price: 10 eur
data allowance: 5 eur
rule result: price + allowance"#;
    let val = eval_rule(code, "t", "result", HashMap::new());
    assert!(
        val.contains("15") && val.contains("eur"),
        "Expected 15 eur, got: {}",
        val
    );
}

#[test]
fn same_family_siblings() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data income: money -> minimum 0 eur
data expense: money -> minimum 0 eur
data salary: 3000 eur
data rent: 1200 eur
rule remaining: salary - rent"#;
    let val = eval_rule(code, "t", "remaining", HashMap::new());
    assert!(
        val.contains("1800") && val.contains("eur"),
        "Expected 1800 eur, got: {}",
        val
    );
}

#[test]
fn same_family_result_used_in_comparison() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data budget: money -> unit jpy 160.00 -> minimum 0 eur
data price: 4 eur
data fee: 5 eur
data limit: 8 eur
rule total: price + fee
rule over_budget: total > limit"#;
    assert_eq!(eval_rule(code, "t", "over_budget", HashMap::new()), "true");
}

// ═══════════════════════════════════════════════════════════════════
// Quantity / Quantity → Number (dimensionless)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_divide_quantity_returns_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data total: 10 eur
data unit_price: 5 eur
rule ratio: total / unit_price"#;
    let val = eval_rule(code, "t", "ratio", HashMap::new());
    assert!(
        val == "2" || val == "2.00" || val.starts_with("2.0"),
        "10 eur / 5 eur should be dimensionless 2, got: {}",
        val
    );
    assert!(
        !val.to_lowercase().contains("eur"),
        "10 eur / 5 eur should NOT contain unit, got: {}",
        val
    );
}

#[test]
fn quantity_divide_quantity_result_usable_as_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data revenue: 100 eur
data cost: 50 eur
rule margin_factor: revenue / cost
rule doubled: margin_factor * 10"#;
    let val = eval_rule(code, "t", "doubled", HashMap::new());
    assert!(
        val == "20" || val.starts_with("20"),
        "margin_factor=2, doubled=20, got: {}",
        val
    );
    assert!(
        !val.to_lowercase().contains("eur"),
        "number * number should not have unit, got: {}",
        val
    );
}

// ═══════════════════════════════════════════════════════════════════
// Number / Quantity → Number (dimensionless)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn number_divide_quantity_returns_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data count: 20
data price: 10 eur
rule units_per_eur: count / price"#;
    let val = eval_rule(code, "t", "units_per_eur", HashMap::new());
    assert!(
        val == "2" || val == "2.00" || val.starts_with("2.0"),
        "20 / 10 eur should be dimensionless 2, got: {}",
        val
    );
    assert!(
        !val.to_lowercase().contains("eur"),
        "number / quantity should NOT contain unit, got: {}",
        val
    );
}

// ═══════════════════════════════════════════════════════════════════
// Quantity * Quantity → rejected at plan time
// Use `(a as number) * (b as number)` to multiply quantity values.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_multiply_quantity_rejected_at_plan_time() {
    let mut engine = Engine::new();
    let result = engine.load(
        r#"spec t
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule product: a * b"#,
        lemma::SourceType::Volatile,
    );
    assert!(
        result.is_err(),
        "Quantity * Quantity should be rejected at plan time"
    );
}

#[test]
fn quantity_multiply_quantity_via_as_number_produces_number() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule product: (a as number) * (b as number)"#;
    let val = eval_rule(code, "t", "product", HashMap::new());
    assert!(
        val.contains("50"),
        "(a as number) * (b as number) should be 50, got: {}",
        val
    );
    assert!(
        !val.to_lowercase().contains("eur"),
        "result should not have unit, got: {}",
        val
    );
}
