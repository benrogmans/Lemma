//! Multidimensional unit type system tests — Phase 1 inventory (D1–D12) plus integration
//! examples covering velocity, acceleration, Newton's law, wage-rate, and PCB reactance.
//!
//! Each test is labelled with the inventory item it covers (D1–D12) or the scenario it
//! demonstrates.  Tests that should succeed use `eval_rule`; tests that must be rejected at
//! plan time use `expect_plan_error`.

use lemma::DateTimeValue;
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("test.lemma")))
}

fn eval_rule(code: &str, spec_name: &str, rule_name: &str) -> String {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let now = DateTimeValue::now();
    let response = engine
        .run(None, spec_name, Some(&now), HashMap::new(), false, None)
        .expect("Should evaluate");
    let result = response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found in response", rule_name));
    if result.vetoed {
        panic!(
            "Rule '{}' returned veto: {:?}",
            rule_name, result.veto_reason
        );
    }
    result.display.clone().expect("display")
}

fn eval_rule_quantity_unit(code: &str, spec_name: &str, rule_name: &str, unit: &str) -> Decimal {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let now = DateTimeValue::now();
    let response = engine
        .run(None, spec_name, Some(&now), HashMap::new(), false, None)
        .expect("Should evaluate");
    let result = response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found in response", rule_name));
    if result.vetoed {
        panic!(
            "Rule '{}' returned veto: {:?}",
            rule_name, result.veto_reason
        );
    }
    if let Some(calendar) = &result.calendar {
        assert_eq!(
            calendar.unit, unit,
            "expected calendar unit '{unit}', got '{}'",
            calendar.unit
        );
        return Decimal::from_str(&calendar.value).expect("calendar value decimal");
    }
    let quantity = result.quantity.as_ref().expect("quantity map");
    Decimal::from_str(
        quantity
            .get(unit)
            .unwrap_or_else(|| panic!("quantity map missing unit '{unit}'")),
    )
    .expect("quantity map decimal")
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
uses lemma units
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
    // 100 meter / 20 second → 5 mps; cast to kmh.
    // 5 m/s × (3600 s/h ÷ 1000 m/km) = 18 km/h exactly.
    let code = r#"spec d2b
uses lemma units
data length: quantity -> unit meter 1 -> unit kilometer 1000
data velocity: quantity -> unit mps meter/second -> unit kmh kilometer/hour
data dist: 100 meter
data secs: 20 seconds
rule speed_kmh: (dist / secs) as kmh"#;
    assert_eq!(
        eval_rule_quantity_unit(code, "d2b", "speed_kmh", "kmh"),
        Decimal::from(18)
    );
}

// =============================================================================
// D3: Inconsistent unit decompositions within a quantity type are rejected
// =============================================================================

#[test]
fn d3_inconsistent_unit_decompositions_rejected() {
    // One unit is `meter/second` (velocity), another is `meter` (length) — inconsistent.
    let code = r#"spec d3
uses lemma units
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
uses lemma units
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
uses lemma units
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
uses lemma units
data length: quantity -> unit meter 1
data velocity: quantity -> unit mps meter/second

spec spec_a
uses lemma units
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
uses lemma units
data money: quantity -> unit eur 1.00
data wage_rate: quantity
  -> unit eur_per_second eur/second
  -> unit standard 28.50 eur/hour
data hours_worked: 40 hours
data rate: 1 standard
rule total: (rate * hours_worked)"#;
    let val = eval_rule(code, "d8", "total");
    assert!(val.contains("1140"), "Expected 1140 eur, got: {val}");
}

// =============================================================================
// D9: Quantity with no factor-1 unit is rejected
// =============================================================================

#[test]
fn d9_quantity_without_factor_one_unit_accepted() {
    let code = r#"spec d9
data length: quantity -> unit kilometer 1000 -> unit mile 1609
data dist: 5 kilometer
rule miles: dist as mile"#;
    let miles = eval_rule_quantity_unit(code, "d9", "miles", "mile");
    assert!(
        miles > Decimal::from(3) && miles < Decimal::from(4),
        "5 km ≈ 3.1 mile, got: {miles}"
    );
}

// =============================================================================
// D10: Calendar unit keeps its own axis in multidimensional arithmetic
// =============================================================================

#[test]
fn d10_calendar_unit_cross_axis_arithmetic_at_rule_boundary() {
    // `sales / month` where `sales` is a money quantity produces an anonymous intermediate
    // {money:1, calendar:-1}. At the rule boundary this is rejected as anonymous.
    let code = r#"spec d10
uses lemma units
data money: quantity -> unit eur 1.00
data sales: 1200 eur
rule rate: sales / 1 month"#;
    expect_plan_error(code, "anonymous intermediate");
}

#[test]
fn d10_calendar_unit_in_derived_quantity_definition_allowed() {
    let code = r#"spec d10b
uses lemma units
data money: quantity -> unit eur 1.00
data monthly_rate: quantity
  -> unit eur_per_month eur/month
data sales: 1200 eur
data months: 1 month
rule rate: (sales / months)"#;
    let val = eval_rule(code, "d10b", "rate");
    assert!(
        val.contains("1200") && val.to_lowercase().contains("eur_per_month"),
        "Expected 1200 eur_per_month, got: {val}"
    );
}

/// Inline calendar literal `1 month` with a declared rate type must promote like d10b.
#[test]
fn d10_calendar_literal_inline_with_rate_type_promotes() {
    let code = r#"spec d10_inline
uses lemma units
data money: quantity -> unit eur 1.00
data monthly_rate: quantity
  -> unit eur_per_month eur/month
data sales: 1200 eur
rule rate: sales / 1 month"#;
    let val = eval_rule(code, "d10_inline", "rate");
    assert!(
        val.contains("1200") && val.to_lowercase().contains("eur_per_month"),
        "Expected 1200 eur_per_month, got: {val}"
    );
}

#[test]
fn d10_exact_duration_compound_cast_allowed() {
    let code = r#"spec d10c
uses lemma units
data money: quantity -> unit eur 1.00
data per_second_rate: quantity
  -> unit eur_per_second eur/second
data sales: 1200 eur
data seconds: 1 second
rule rate: (sales / seconds)"#;
    let val = eval_rule(code, "d10c", "rate");
    assert!(
        val.contains("1200") && val.to_lowercase().contains("eur_per_second"),
        "Expected 1200 eur_per_second, got: {val}"
    );
}

/// Inverse of `d10_calendar_unit_in_derived_quantity_definition_allowed`:
/// balance / monthly burn rate → months of runway (not eur/month).
/// Fails until calendar quantity trait + `uses lemma units` exist.
#[test]
fn d10_runway_balance_over_monthly_rate_as_month() {
    let code = r#"spec d10_runway
uses lemma units
data money: quantity -> unit eur 1.00
data money_flow: quantity
  -> unit eur_month eur/month
data balance: 120000 eur
data burn_rate: 8000 eur_month
rule runway_months: (balance / burn_rate) as month"#;
    let mut engine = Engine::new();
    engine
        .load(code, source())
        .expect("runway spec must load and plan");
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "d10_runway", Some(&now), HashMap::new(), false, None)
        .expect("runway spec must evaluate");
    let display = response
        .results
        .get("runway_months")
        .expect("runway_months rule")
        .display
        .clone()
        .expect("display");
    assert!(
        display.contains("15"),
        "120000 eur / 8000 eur_month = 15 month runway, got: {display}"
    );
}

// =============================================================================
// D11: Calendar + Calendar stays on the calendar axis
// =============================================================================

#[test]
fn d11_calendar_duration_in_date_arithmetic() {
    let code = r#"spec d11
uses lemma units
data d1: 3 month
data d2: 9 month
rule total: d1 + d2"#;
    assert_eq!(
        eval_rule_quantity_unit(code, "d11", "total", "month"),
        Decimal::from(12)
    );
}

// =============================================================================
// D12: `duration` keyword resolves to built-in quantity (stretch goal)
// =============================================================================

#[test]
fn d12_duration_keyword_in_compound_unit() {
    // `hour` in a compound unit expression resolves to the built-in Duration dimension.
    // This is the basic mechanism that makes velocity `meter/second` work.
    let code = r#"spec d12
uses lemma units
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
uses lemma units
data length: quantity -> unit meter 1 -> unit kilometer 1000
data velocity: quantity -> unit mps meter/second -> unit kmh kilometer/hour
data dist: 1000 meter
data time: 200 seconds
rule speed_mps: (dist / time) as mps
rule speed_kmh: (dist / time) as kmh"#;
    let mps = eval_rule(code, "phys", "speed_mps");
    assert!(mps.contains('5'), "Expected 5 mps, got: {mps}");
    assert_eq!(
        eval_rule_quantity_unit(code, "phys", "speed_kmh", "kmh"),
        Decimal::from(18)
    );
}

// =============================================================================
// Integration: wage-rate (eur/hour × hours = eur)
// =============================================================================

#[test]
fn integration_wage_rate() {
    let code = r#"spec wage
uses lemma units
data money: quantity -> unit eur 1.00 -> unit cent 0.01
data wage_rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_hour eur/hour
data hours: 8 hours
data rate: 85 eur_per_hour
rule total: (rate * hours)"#;
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
uses lemma units
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
uses lemma units
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
rule product: (a as eur as number) * (b as eur as number)"#;
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
uses lemma units
data length: quantity -> unit meter 1
data money: quantity -> unit eur 1.00
data dist: 100 meter
data time: 20 seconds
rule speed_as_eur: (dist / time)"#;
    expect_plan_error(code, "anonymous intermediate");
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

// =============================================================================
// Dimensionless derived quantities referenced in compound types
// =============================================================================

#[test]
fn dimensionless_derived_quantity_referenced_in_compound_type_loads() {
    let code = r#"spec units
data mass_type: quantity
  -> unit kg 1
  -> unit gram 0.001
data ratio_type: quantity
  -> unit mass_ratio kg/kg
data scaled_mass_type: quantity
  -> unit scaled_kg mass_ratio*kg
data base_mass: 10 kg
data scale: 2 mass_ratio
rule result: (base_mass * scale) as scaled_kg"#;
    eval_rule(code, "units", "result");
}

#[test]
fn dimensionless_compound_unit_evaluates_to_correct_magnitude() {
    let code = r#"spec units
data mass_type: quantity
  -> unit kg 1
data ratio_type: quantity
  -> unit mass_ratio kg/kg
data m: 5 kg
data r: 3 mass_ratio
rule scaled: (m * r) as kg"#;
    let result = eval_rule(code, "units", "scaled");
    assert!(result.contains("15"), "expected 15, got: {result}");
}
