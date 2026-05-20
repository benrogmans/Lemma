//! Derived quantity unit planning: `uses lemma si` duration units, compound-of-compound
//! resolution, topological ordering, and conversion-factor normalization.

use lemma::Engine;
use std::path::PathBuf;
use std::sync::Arc;

fn path_source(file: &str) -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from(file)))
}

fn load_ok(code: &str, source_file: &str, message: &str) {
    let mut engine = Engine::new();
    engine.load(code, path_source(source_file)).expect(message);
}

fn expect_plan_error(code: &str, source_file: &str, expected_fragment: &str) {
    let mut engine = Engine::new();
    let result = engine.load(code, path_source(source_file));
    assert!(
        result.is_err(),
        "expected planning to fail containing '{expected_fragment}'"
    );
    if expected_fragment.is_empty() {
        return;
    }
    let errors = result.unwrap_err();
    let combined = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        combined.contains(expected_fragment),
        "expected error containing '{expected_fragment}', got: {combined}"
    );
}

/// Mirrors `cli/tests/integrations/examples/03_spec_references.lemma` contractor block:
/// compound wage-rate units reference `second` and `hour` from `si.duration` via `uses lemma si`.
#[test]
fn uses_lemma_compound_wage_rate_units_plan_without_unknown_unit_error() {
    let code = r#"spec contractor
uses lemma si

data money: quantity
  -> unit eur 1.00

data wage_rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_hour eur/hour

rule smoke: true
"#;
    load_ok(
        code,
        "contractor.lemma",
        "planning must accept eur/second and eur/hour when uses lemma si brings duration units",
    );
}

/// Fleet motor: EUR/hour premium rate, then EUR/hour per insured vehicle. Exercises compound
/// factors that name a unit from another derived quantity (`eur_per_hour`) plus `vehicle`.
#[test]
fn uses_lemma_insurance_premium_per_vehicle_compound_plans() {
    let code = r#"spec fleet_motor_quote
uses lemma si

data money: quantity
  -> unit eur 1.00

data fleet_unit: quantity
  -> unit vehicle 1.00

data premium_rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_hour eur/hour

data premium_per_vehicle: quantity
  -> unit eur_hour_per_vehicle eur_per_hour/vehicle

rule smoke: true
"#;
    load_ok(
        code,
        "fleet_motor_quote.lemma",
        "compound unit may name derived units from an earlier derived quantity (eur_per_hour/vehicle)",
    );
}

/// Force = mass * acceleration; `newton` references compound unit `mps2`.
#[test]
fn compound_newton_force_unit_plans() {
    let code = r#"spec mechanics
uses lemma si

data mass: quantity
  -> unit kg 1
  -> unit gram 0.001

data length: quantity
  -> unit meter 1
  -> unit km 1000

data acceleration: quantity
  -> unit mps2 meter/second^2

data force: quantity
  -> unit newton kg * mps2

rule smoke: true
"#;
    load_ok(
        code,
        "mechanics_newton.lemma",
        "newton = kg * mps2 must plan with compound-of-compound normalization",
    );
}

/// Pressure = force / area; `pascal` references `newton` and `sqm` (depth-3 compound chain).
#[test]
fn compound_pascal_pressure_unit_plans() {
    let code = r#"spec mechanics_pressure
uses lemma si

data mass: quantity
  -> unit kg 1

data length: quantity
  -> unit meter 1

data acceleration: quantity
  -> unit mps2 meter/second^2

data force: quantity
  -> unit newton kg * mps2

data area: quantity
  -> unit sqm meter^2

data pressure: quantity
  -> unit pascal newton/sqm

rule smoke: true
"#;
    load_ok(
        code,
        "mechanics_pascal.lemma",
        "pascal = newton/sqm must plan across three derived quantity layers",
    );
}

/// Population density with two compound units and different area denominators.
#[test]
fn compound_population_density_multi_unit_plans() {
    let code = r#"spec demography
uses lemma si

data population: quantity
  -> unit person 1

data length: quantity
  -> unit meter 1
  -> unit km 1000

data area: quantity
  -> unit sqm meter^2
  -> unit sqkm km^2

data density: quantity
  -> unit per_sqm person/sqm
  -> unit per_sqkm person/sqkm

rule smoke: true
"#;
    load_ok(
        code,
        "demography_density.lemma",
        "person/sqm and person/sqkm must plan with distinct conversion factors",
    );
}

/// Growth rate: compound-of-compound with one unit naturally canonical (factor 1).
#[test]
fn compound_annual_growth_rate_multi_unit_plans() {
    let code = r#"spec growth
uses lemma si

data money: quantity
  -> unit eur 1

data rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_hour eur/hour

data annual_growth: quantity
  -> unit growth_per_hour eur_per_hour/eur
  -> unit growth_per_second eur_per_second/eur

rule smoke: true
"#;
    load_ok(
        code,
        "growth_cagr.lemma",
        "eur_per_hour/eur and eur_per_second/eur must plan without spurious normalization",
    );
}

/// Three-level compound chain: each type has a single compound unit requiring normalization.
#[test]
fn compound_three_level_chain_plans() {
    let code = r#"spec chain
uses lemma si

data a: quantity
  -> unit au 1

data b: quantity
  -> unit bu au/second

data c: quantity
  -> unit cu bu/au

data d: quantity
  -> unit du cu/au

rule smoke: true
"#;
    load_ok(
        code,
        "compound_chain.lemma",
        "four-level compound chain must plan with successive normalization",
    );
}

/// Circular derived quantity dependency must be rejected at plan time.
#[test]
fn compound_cycle_between_quantity_types_rejected() {
    let code = r#"spec cycle
uses lemma si

data x: quantity
  -> unit xu yu/second

data y: quantity
  -> unit yu xu/second

rule smoke: true
"#;
    expect_plan_error(
        code,
        "compound_cycle.lemma",
        "circular compound quantity type dependency",
    );
}

/// Explicit prefix on a compound unit alongside another compound unit (normalization).
#[test]
fn compound_kilonewton_with_prefix_plans() {
    let code = r#"spec force_prefix
uses lemma si

data mass: quantity
  -> unit kg 1
  -> unit gram 0.001

data length: quantity
  -> unit meter 1
  -> unit km 1000

data acceleration: quantity
  -> unit mps2 meter/second^2
  -> unit kmh2 km/hour^2

data force: quantity
  -> unit newton kg * mps2
  -> unit kilonewton 1000 kg * meter/second^2

rule smoke: true
"#;
    load_ok(
        code,
        "force_prefix.lemma",
        "newton and kilonewton compound units must plan with normalization",
    );
}

/// Convert 1 kilonewton to gram * km/hour^2 via a third compound unit in the same type.
/// 1 newton = 1 kg * m/s^2.
/// 1 gram_kmh2 = 0.001 kg * 1000 m / (3600 s)^2 = 1 kg*m/s^2 * (0.001 * 1000 / 3600^2)
///             = 1/12960000 newton.
/// 1 kilonewton = 1000 newton = 1000 * 12960000 gram_kmh2 = 12960000000 gram_kmh2.
#[test]
fn compound_kilonewton_to_gram_kmh2_conversion() {
    let code = r#"spec force_conv
uses lemma si

data mass: quantity
  -> unit kg 1
  -> unit gram 0.001

data length: quantity
  -> unit meter 1
  -> unit km 1000

data acceleration: quantity
  -> unit mps2 meter/second^2
  -> unit kmh2 km/hour^2

data force: quantity
  -> unit newton kg * mps2
  -> unit kilonewton 1000 kg * meter/second^2
  -> unit gram_kmh2 gram * kmh2

rule converted: 1 kilonewton as gram_kmh2
"#;
    let mut engine = Engine::new();
    engine
        .load(code, path_source("force_conv.lemma"))
        .expect("newton/kilonewton/gram_kmh2 must plan");
    let now = lemma::parsing::ast::DateTimeValue::now();
    let response = engine
        .run(
            None,
            "force_conv",
            Some(&now),
            std::collections::HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("should evaluate");
    let result = response
        .results
        .get("converted")
        .expect("rule 'converted' not found");
    let val = result
        .result
        .value()
        .unwrap_or_else(|| panic!("rule 'converted' returned non-value: {:?}", result.result))
        .to_string();
    assert!(
        val.contains("12960000000") && val.to_lowercase().contains("gram_kmh2"),
        "expected 12960000000 gram_kmh2, got: {val}"
    );
}

/// Two compound volume units: `cubic_meter` natural canonical, `liter` explicit prefix on same expression.
#[test]
fn compound_volume_liter_and_cubic_meter_plans() {
    let code = r#"spec volume
uses lemma si

data length: quantity
  -> unit meter 1
  -> unit km 1000

data volume: quantity
  -> unit cubic_meter meter^3
  -> unit liter 0.001 meter^3

rule smoke: true
"#;
    load_ok(
        code,
        "volume.lemma",
        "liter and cubic_meter must coexist with cubic_meter as natural canonical",
    );
}
