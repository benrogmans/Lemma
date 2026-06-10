//! Tests for Quantity/Quantity and Duration/Duration arithmetic result types,
//! Quantity*Quantity / Duration*Duration rejection, and `as number` conversion.

use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

fn load_ok(code: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("Should parse and plan");
    engine
}

fn load_err(code: &str) -> String {
    let mut engine = Engine::new();
    let errs = engine
        .load(code, lemma::SourceType::Volatile)
        .expect_err("Should fail to plan");
    errs.iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

fn eval(engine: &Engine, spec: &str, rule: &str, data: HashMap<String, String>) -> String {
    let now = DateTimeValue::now();
    let response = engine
        .run(None, spec, Some(&now), data, false, None)
        .expect("Should evaluate");
    response
        .results
        .get(rule)
        .unwrap_or_else(|| panic!("Rule '{}' should exist", rule))
        .display
        .clone()
        .expect("display")
}

// ═══════════════════════════════════════════════════════════════════
// Quantity / Quantity → Number
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_divide_quantity_same_type_returns_number() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule result: a / b"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "2", "10 eur / 5 eur = 2 (dimensionless), got: {}", val);
}

#[test]
fn quantity_divide_quantity_result_is_not_quantity() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule result: a / b"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert!(
        !val.to_lowercase().contains("eur"),
        "10 eur / 5 eur should be dimensionless, not contain unit, got: {}",
        val
    );
}

// ═══════════════════════════════════════════════════════════════════
// Quantity * Quantity → rejected by planner
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_multiply_quantity_same_family_rejected_by_planner() {
    let err = load_err(
        r#"spec t
uses lemma units
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule result: a * b"#,
    );
    assert!(
        !err.is_empty(),
        "Quantity * Quantity should be rejected at plan time"
    );
}

#[test]
fn quantity_multiply_quantity_via_as_number_allowed() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule result: (a as eur as number) * (b as eur as number)"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "50", "10 * 5 = 50, got: {}", val);
}

// ═══════════════════════════════════════════════════════════════════
// Duration / Duration → Number
// ═══════════════════════════════════════════════════════════════════

#[test]
fn duration_divide_duration_returns_number() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data a: 10 hours
data b: 5 hours
rule result: a / b"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "2", "10 hours / 5 hours = 2, got: {}", val);
}

#[test]
fn duration_divide_duration_cross_unit_returns_number() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data a: 2 hours
data b: 30 minutes
rule result: a / b"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "4", "2 hours / 30 minutes = 4, got: {}", val);
}

#[test]
fn duration_divide_duration_result_is_not_duration() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data a: 10 hours
data b: 5 hours
rule result: a / b"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert!(
        !val.to_lowercase().contains("hour"),
        "Duration / Duration should be dimensionless, got: {}",
        val
    );
}

// ═══════════════════════════════════════════════════════════════════
// Duration * Duration → rejected by planner
// ═══════════════════════════════════════════════════════════════════

#[test]
fn duration_multiply_duration_rejected_by_planner() {
    let err = load_err(
        r#"spec t
uses lemma units
data a: 5 hours
data b: 3 hours
rule result: a * b"#,
    );
    assert!(
        !err.is_empty(),
        "Duration * Duration should be rejected at plan time"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Number / Duration → Number (not Duration)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn number_divide_duration_returns_number_not_duration() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data n: 10
data d: 5 hours
rule result: n / d"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "2", "10 / 5 hours = 2, got: {}", val);
    assert!(
        !val.to_lowercase().contains("hour"),
        "number / duration should be dimensionless, got: {}",
        val
    );
}

#[test]
fn number_multiply_duration_returns_duration() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data n: 3
data d: 5 hours
rule result: n * d"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert!(val.contains("15"), "3 * 5 hours = 15 hours, got: {}", val);
    assert!(
        val.to_lowercase().contains("hour"),
        "number * duration should be duration, got: {}",
        val
    );
}

// ═══════════════════════════════════════════════════════════════════
// Duration op Number → Duration (regression guard)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn duration_divide_number_returns_duration() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data d: 10 hours
data n: 2
rule result: d / n"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert!(val.contains("5"), "10 hours / 2 = 5 hours, got: {}", val);
    assert!(
        val.to_lowercase().contains("hour"),
        "duration / number should stay duration, got: {}",
        val
    );
}

#[test]
fn duration_modulo_number_returns_duration() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data d: 7 hours
data n: 3
rule result: d % n"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert!(val.contains("1"), "7 hours % 3 = 1 hour, got: {}", val);
    assert!(
        val.to_lowercase().contains("hour"),
        "duration % number should stay duration, got: {}",
        val
    );
}

// ═══════════════════════════════════════════════════════════════════
// Quantity ^ Ratio and Quantity % Ratio → rejected (crash prevention)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_power_ratio_rejected_by_planner() {
    let err = load_err(
        r#"spec t
uses lemma units
data money: quantity -> unit eur 1.00
data a: 100 eur
data r: 50%
rule result: a ^ r"#,
    );
    assert!(
        !err.is_empty(),
        "Quantity ^ Ratio should be rejected at plan time"
    );
}

#[test]
fn quantity_modulo_ratio_rejected_by_planner() {
    let err = load_err(
        r#"spec t
uses lemma units
data money: quantity -> unit eur 1.00
data a: 100 eur
data r: 25%
rule result: a % r"#,
    );
    assert!(
        !err.is_empty(),
        "Quantity % Ratio should be rejected at plan time"
    );
}

// ═══════════════════════════════════════════════════════════════════
// `as number` conversion
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_as_number_strips_unit() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data money: quantity -> unit eur 1.00
data a: 10 eur
rule result: a as eur as number"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "10", "10 eur as number = 10, got: {}", val);
}

#[test]
fn quantity_as_number_result_is_usable_as_number() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule result: (a as eur as number) * (b as eur as number)"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "50", "10 * 5 = 50, got: {}", val);
}

#[test]
fn duration_as_number_strips_unit() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data d: 5 hours
rule result: d as hours as number"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "5", "5 hours as number = 5, got: {}", val);
}

#[test]
fn ratio_as_number_strips_unit() {
    let engine = load_ok(
        r#"spec t
uses lemma units
data r: 25%
rule result: r as number"#,
    );
    let val = eval(&engine, "t", "result", HashMap::new());
    assert_eq!(val, "0.25", "25% as number = 0.25, got: {}", val);
}
