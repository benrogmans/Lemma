//! Multidimensional unit type system tests — Phase 1 inventory (D1–D12) plus integration
//! examples covering velocity, acceleration, Newton's law, wage-rate, and PCB reactance.
//!
//! Each test is labelled with the inventory item it covers (D1–D12) or the scenario it
//! demonstrates.  Tests that should succeed use `eval_rule`; tests that must be rejected at
//! plan time use `expect_plan_error`.

use lemma::parsing::ast::DateTimeValue;
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
        .run(
            None,
            spec_name,
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("Should evaluate");
    let result = response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found in response", rule_name));
    result
        .result
        .value()
        .unwrap_or_else(|| {
            panic!(
                "Rule '{}' returned non-value: {:?}",
                rule_name, result.result
            )
        })
        .to_string()
}

fn expect_plan_error(code: &str, expected_fragment: &str) {
    let mut engine = Engine::new();
    let result = engine.load(code, source());
    assert!(
        result.is_err(),
        "Expected planning error containing '{}', but loading succeeded",
        expected_fragment
    );
    if !expected_fragment.is_empty() {
        let errors = result.unwrap_err();
        let combined = errors
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
}

// =============================================================================
// D1: Base quantity decomposition — each base quantity gets {quantity_name: 1}
// =============================================================================

#[test]
fn d1_base_quantity_decomposition_self_cancels() {
    // Same-family Quantity / Quantity cancels to dimensionless Number.
    let code = r#"spec d1
data money: quantity -> unit eur 1.00 -> unit cent 0.01
data price1: 10 eur
data price2: 5 eur
rule ratio: price1 / price2"#;
    let val = eval_rule(code, "d1", "ratio");
    assert!(val.contains('2'), "Expected 2 (10/5), got: {val}");
    // The result must be a plain number, not a quantity value.
    assert!(
        !val.to_lowercase().contains("eur"),
        "Same-family quantity/quantity should cancel to number, got: {val}"
    );
}

#[test]
fn d1_cross_unit_same_family_cancels() {
    // 100 cent / 1 eur — cross-unit within the same family, should still cancel to Number.
    let code = r#"spec d1b
data money: quantity -> unit eur 1.00 -> unit cent 0.01
data price_cents: 100 cent
data price_eur: 1 eur
rule ratio: price_cents / price_eur"#;
    let val = eval_rule(code, "d1b", "ratio");
    assert!(
        val.contains('1'),
        "Expected 1.00 (100 cent == 1 eur), got: {val}"
    );
}

// =============================================================================
// D2: Velocity decomposition from compound unit expression
// =============================================================================

#[test]
fn d2_velocity_compound_unit_decomposition() {
    // `mps` has decomposition {length:1, duration:-1}.
    // 100 meter / 20 second → anonymous {length:1, duration:-1} → `as mps` → 5 mps.
    let code = r#"spec d2
uses lemma si
data length: quantity -> unit meter 1 -> unit kilometer 1000
data velocity: quantity -> unit mps meter/second -> unit kmh kilometer/hour
data dist: 100 meter
data secs: 20 seconds
rule speed: (dist / secs) as mps"#;
    let val = eval_rule(code, "d2", "speed");
    assert!(val.contains('5'), "Expected 5 mps, got: {val}");
    assert!(
        val.to_lowercase().contains("mps"),
        "Result should be in mps, got: {val}"
    );
}

#[test]
fn d2_velocity_in_kmh_conversion() {
    // 100 meter / 20 second → 5 mps; cast to kmh (1 kmh = 1000/3600 mps = ~0.2778 mps).
    // 5 mps / 0.2778... mps/kmh = 18 kmh (approximately, due to decimal precision).
    let code = r#"spec d2b
uses lemma si
data length: quantity -> unit meter 1 -> unit kilometer 1000
data velocity: quantity -> unit mps meter/second -> unit kmh kilometer/hour
data dist: 100 meter
data secs: 20 seconds
rule speed_kmh: (dist / secs) as kmh"#;
    let val = eval_rule(code, "d2b", "speed_kmh");
    assert!(
        val.contains("17") || val.contains("18"),
        "Expected ~18 kmh, got: {val}"
    );
    assert!(
        val.to_lowercase().contains("kmh"),
        "Result should be in kmh, got: {val}"
    );
}

// =============================================================================
// D3: Inconsistent unit decompositions within a quantity type are rejected
// =============================================================================

#[test]
fn d3_inconsistent_unit_decompositions_rejected() {
    // One unit is `meter/second` (velocity), another is `meter` (length) — inconsistent.
    let code = r#"spec d3
uses lemma si
data length: quantity -> unit meter 1
data velocity: quantity -> unit mps meter/second -> unit just_meters meter"#;
    expect_plan_error(code, "inconsistent");
}

// =============================================================================
// D4: Same-quantity unit reference is rejected
// =============================================================================

#[test]
fn d4_same_quantity_self_reference_rejected() {
    // A quantity type cannot use its own units in a compound expression.
    // When the reference is to the type name rather than a unit name, the error is
    // "not a known unit" (the type name is not registered as a unit).
    // When the reference is to a unit of the same type, it's "same quantity type".
    // Both cases are rejected at plan time.
    let code = r#"spec d4
uses lemma si
data velocity: quantity -> unit mps velocity/second"#;
    expect_plan_error(code, "");
}

// =============================================================================
// D5: `uses` does NOT contribute base quantity types for compound unit declarations
// =============================================================================

#[test]
fn d5_uses_does_not_import_quantity_types_for_compound_units() {
    // `uses spec_b` only imports rule references and data binding — not type definitions.
    // Compound unit `meter/second` in `velocity` requires `meter` to be an in-scope quantity unit,
    // but `uses` only brings in spec references, so this should fail.
    let code = r#"spec spec_b
data length: quantity -> unit meter 1

spec spec_a
uses lb: spec_b
data velocity: quantity -> unit mps meter/second"#;
    expect_plan_error(code, "");
}

// =============================================================================
// D6: `data x: spec_b.TypeName` after `uses` makes the base type available for compound declarations
// =============================================================================

#[test]
fn d6_uses_and_qualified_parent_makes_type_available_for_compound_units() {
    // `uses spec_b` plus `data length: spec_b.length` imports the `length` type (including its `meter` unit)
    // and makes `meter` available for compound unit expressions in `spec_a`.
    let code = r#"spec spec_b
data length: quantity -> unit meter 1

spec spec_a
uses lemma si
uses spec_b
data length: spec_b.length
data velocity: quantity -> unit mps meter/second
data dist: 100 meter
data secs: 20 seconds
rule speed: (dist / secs) as mps"#;
    let val = eval_rule(code, "spec_a", "speed");
    assert!(val.contains('5'), "Expected 5 mps, got: {val}");
}

// =============================================================================
// D7: Cross-library same-named `length` still resolves compound cast when velocity is imported
// =============================================================================

#[test]
fn d7_cross_library_same_named_quantity_resolves_speed_literal() {
    // Two `length` types from different specs: `spec_a` defines its own `length` and
    // imports `velocity` from `spec_b`. Anonymous `dist / secs` must still cast to
    // the imported `mps` unit; planning and evaluation complete without error.
    let code = r#"spec spec_b
uses lemma si
data length: quantity -> unit meter 1
data velocity: quantity -> unit mps meter/second

spec spec_a
uses lemma si
data length: quantity -> unit meter 1
uses spec_b_ref: spec_b
data velocity: spec_b_ref.velocity
data dist: 100 meter
data secs: 20 seconds
rule speed: (dist / secs) as mps"#;
    let val = eval_rule(code, "spec_a", "speed");
    assert!(
        val.contains('5'),
        "Expected speed near 5 mps for 100 meter / 20 seconds, got: {val}"
    );
    assert!(
        val.to_lowercase().contains("mps"),
        "Result should be in mps, got: {val}"
    );
}

// =============================================================================
// D8: Constant-as-named-unit resolves (wage standard = 28.50 eur/hour)
// =============================================================================

#[test]
fn d8_compound_unit_with_numeric_prefix() {
    // `unit standard 28.50 eur/second` means 1 standard = 28.50 canonical (eur/second).
    // Wait — the correct modelling is: 1 standard represents 28.50 eur/hour, so canonical
    // factor = 28.50 * (1/3600) = 28.50/3600 eur/second.
    // We define: unit eur_per_second eur/second (canonical, factor=1)
    //            unit standard 28.50 eur/hour   (factor = 28.50/3600)
    // 40 hours * standard rate = 40 * 3600 s * (28.50/3600 eur/s) = 40 * 28.50 = 1140 eur.
    let code = r#"spec d8
uses lemma si
data money: quantity -> unit eur 1.00
data wage_rate: quantity
  -> unit eur_per_second eur/second
  -> unit standard 28.50 eur/hour
data hours_worked: 40 hours
data rate: 1 standard
rule total: (rate * hours_worked) as eur"#;
    let val = eval_rule(code, "d8", "total");
    assert!(val.contains("1140"), "Expected 1140 eur, got: {val}");
}

// =============================================================================
// D9: Quantity with no factor-1 unit is rejected
// =============================================================================

#[test]
fn d9_quantity_with_no_canonical_unit_rejected() {
    // Every quantity type must have exactly one unit with factor 1 (the canonical unit).
    // A quantity that has only non-1 factors is rejected.
    let code = r#"spec d9
data length: quantity -> unit kilometer 1000 -> unit mile 1609"#;
    expect_plan_error(code, "conversion factor 1");
}

// =============================================================================
// D10: Calendar unit keeps its own axis in multidimensional arithmetic
// =============================================================================

#[test]
fn d10_calendar_unit_cross_axis_arithmetic_at_rule_boundary() {
    // `sales / month` where `sales` is a money quantity produces an anonymous intermediate
    // {money:1, calendar:-1}. At the rule boundary this is rejected as anonymous.
    let code = r#"spec d10
data money: quantity -> unit eur 1.00
data sales: 1200 eur
rule rate: sales / 1 month"#;
    expect_plan_error(code, "anonymous intermediate");
}

#[test]
fn d10_calendar_unit_in_derived_quantity_definition_allowed() {
    let code = r#"spec d10b
data money: quantity -> unit eur 1.00
data monthly_rate: quantity
  -> unit eur_per_month eur/month
data sales: 1200 eur
data months: 1 month
rule rate: (sales / months) as eur_per_month"#;
    let val = eval_rule(code, "d10b", "rate");
    assert!(
        val.contains("1200") && val.to_lowercase().contains("eur_per_month"),
        "Expected 1200 eur_per_month, got: {val}"
    );
}

#[test]
fn d10_exact_duration_compound_cast_allowed() {
    let code = r#"spec d10c
uses lemma si
data money: quantity -> unit eur 1.00
data per_second_rate: quantity
  -> unit eur_per_second eur/second
data sales: 1200 eur
data seconds: 1 second
rule rate: (sales / seconds) as eur_per_second"#;
    let val = eval_rule(code, "d10c", "rate");
    assert!(
        val.contains("1200"),
        "Expected 1200 eur_per_second, got: {val}"
    );
}

// =============================================================================
// D11: Calendar + Calendar stays on the calendar axis
// =============================================================================

#[test]
fn d11_calendar_duration_in_date_arithmetic() {
    let code = r#"spec d11
data d1: 3 months
data d2: 9 months
rule total: d1 + d2"#;
    let val = eval_rule(code, "d11", "total");
    assert!(val.contains("12"), "Expected 12 months, got: {val}");
}

// =============================================================================
// D12: `duration` keyword resolves to built-in quantity (stretch goal)
// =============================================================================

#[test]
fn d12_duration_keyword_in_compound_unit() {
    // `hour` in a compound unit expression resolves to the built-in Duration dimension.
    // This is the basic mechanism that makes velocity `meter/second` work.
    let code = r#"spec d12
uses lemma si
data length: quantity -> unit meter 1
data velocity: quantity -> unit mps meter/second
data dist: 200 meter
data secs: 40 seconds
rule speed: (dist / secs) as mps"#;
    let val = eval_rule(code, "d12", "speed");
    assert!(val.contains('5'), "Expected 5 mps, got: {val}");
}

// =============================================================================
// Integration: velocity
// =============================================================================

#[test]
fn integration_velocity_basic() {
    let code = r#"spec phys
uses lemma si
data length: quantity -> unit meter 1 -> unit kilometer 1000
data velocity: quantity -> unit mps meter/second -> unit kmh kilometer/hour
data dist: 1000 meter
data time: 200 seconds
rule speed_mps: (dist / time) as mps
rule speed_kmh: (dist / time) as kmh"#;
    let mps = eval_rule(code, "phys", "speed_mps");
    assert!(mps.contains('5'), "Expected 5 mps, got: {mps}");
    let kmh = eval_rule(code, "phys", "speed_kmh");
    assert!(
        kmh.contains("17") || kmh.contains("18"),
        "Expected ~18 kmh, got: {kmh}"
    );
}

// =============================================================================
// Integration: wage-rate (eur/hour × hours = eur)
// =============================================================================

#[test]
fn integration_wage_rate() {
    let code = r#"spec wage
uses lemma si
data money: quantity -> unit eur 1.00 -> unit cent 0.01
data wage_rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_hour eur/hour
data hours: 8 hours
data rate: 85 eur_per_hour
rule total: (rate * hours) as eur"#;
    let val = eval_rule(code, "wage", "total");
    // 85 eur/hour × 8 hours = 680 eur
    assert!(val.contains("680"), "Expected 680 eur, got: {val}");
}

// =============================================================================
// Integration: anonymous intermediate prohibited at rule boundary
// =============================================================================

#[test]
fn integration_anonymous_at_rule_boundary_rejected() {
    // `dist / time` without `as mps` produces an anonymous intermediate at the rule boundary.
    let code = r#"spec phys
uses lemma si
data length: quantity -> unit meter 1
data dist: 100 meter
data time: 20 seconds
rule speed: dist / time"#;
    expect_plan_error(code, "anonymous intermediate");
}

#[test]
fn integration_as_number_on_anonymous_rejected() {
    // Using `as number` to strip an anonymous compound is rejected.
    let code = r#"spec phys
uses lemma si
data length: quantity -> unit meter 1
data dist: 100 meter
data time: 20 seconds
rule speed: (dist / time) as number"#;
    expect_plan_error(code, "anonymous intermediate");
}

// =============================================================================
// Integration: same-family quantity * quantity rejected (anonymous at rule boundary)
// =============================================================================

#[test]
fn integration_same_family_quantity_multiply_rejected() {
    // money * money produces {money:2} which is anonymous at rule boundary.
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule product: a * b"#;
    expect_plan_error(code, "");
}

#[test]
fn integration_same_family_quantity_multiply_via_as_number() {
    // (a as number) * (b as number) strips the quantity first — valid.
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 10 eur
data b: 5 eur
rule product: (a as number) * (b as number)"#;
    let val = eval_rule(code, "t", "product");
    assert!(val.contains("50"), "Expected 50, got: {val}");
}

// =============================================================================
// Integration: Quantity / Quantity (same family) → Number (dimensionless)
// =============================================================================

#[test]
fn integration_quantity_divide_quantity_same_family() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00 -> unit cent 0.01
data a: 100 eur
data b: 25 eur
rule ratio: a / b"#;
    let val = eval_rule(code, "t", "ratio");
    assert!(val.contains('4'), "Expected 4, got: {val}");
    assert!(
        !val.to_lowercase().contains("eur"),
        "Result should be dimensionless number, got: {val}"
    );
}

// =============================================================================
// Integration: TypedefCast — anonymous intermediate cast to named quantity unit
// =============================================================================

#[test]
fn integration_typedef_cast_dimension_mismatch_rejected() {
    // Cross-dimension cast is rejected: {length:1, duration:-1} cannot be cast to `money`.
    let code = r#"spec phys
uses lemma si
data length: quantity -> unit meter 1
data money: quantity -> unit eur 1.00
data dist: 100 meter
data time: 20 seconds
rule speed_as_eur: (dist / time) as eur"#;
    expect_plan_error(code, "do not match target dimensions");
}

// =============================================================================
// Integration: Quantity ^ Number with integer literal exponent
// =============================================================================

#[test]
fn integration_quantity_power_integer_literal() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 3 eur
rule cube: a ^ 3"#;
    let val = eval_rule(code, "t", "cube");
    assert!(val.contains("27"), "Expected 27 eur, got: {val}");
}

#[test]
fn integration_quantity_power_fractional_rejected() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 4 eur
rule frac_pow: a ^ 0.5"#;
    expect_plan_error(code, "fractional");
}

#[test]
fn integration_quantity_power_variable_rejected() {
    let code = r#"spec t
data money: quantity -> unit eur 1.00
data a: 4 eur
data exponent: 2
rule powered: a ^ exponent"#;
    expect_plan_error(code, "integer literal");
}
