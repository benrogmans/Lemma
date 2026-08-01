//! Declared measure bounds: input parse enforces `Decimal::MAX_SCALE`; planning accepts valid ℚ bounds.
//!
//! IN: literals in `.lemma` must parse as `Decimal`. Oversize literals are rejected at parse.
//! OUT: show converts per-unit magnitudes to rounded decimal strings.

use lemma::DateTimeValue;
use lemma::Engine;
use lemma::TypeSpecification;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Largest power of ten that parses as a `Decimal` and converts to decimal through planning/show.
const COMMITTABLE_BOUND: &str = "10000000000000000000000000000";

/// Eur minimum so milli per-unit magnitude stays within `Decimal::MAX` (10^25 eur → 10^28 milli).
const COMMITTABLE_EUR_MINIMUM_FOR_MILLI: &str = "1000000000000000000000000";

/// First power of ten above the `Decimal` literal range; rejected at parse, not planning.
const OVERSIZE_LITERAL: &str = "100000000000000000000000000000";

fn decimal_lit(s: &str) -> Decimal {
    Decimal::from_str(s).expect("BUG: test decimal literal must parse")
}

fn load(engine: &mut Engine, code: &str) {
    engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .unwrap_or_else(|errs| {
            let joined = errs
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            panic!("expected load to succeed, got: {joined}");
        });
}

fn load_err(engine: &mut Engine, code: &str) -> String {
    let err = engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .expect_err("expected load failure");
    err.errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn quantity_unit<'a>(spec: &'a TypeSpecification, name: &str) -> &'a lemma::MeasureUnit {
    match spec {
        TypeSpecification::Measure { units, .. } => units
            .get(name)
            .expect("BUG: measure unit must exist in test fixture"),
        _ => panic!("BUG: test fixture must use measure type"),
    }
}

fn measure_spec(constraint: &str) -> String {
    format!(
        r#"
spec t
data money: measure
  -> unit eur 1
  -> {constraint}
rule r: money
"#
    )
}

fn measure_spec_with_milli(constraint: &str) -> String {
    format!(
        r#"
spec t
data money: measure
  -> unit eur 1
  -> unit milli 0.001
  -> {constraint}
rule r: money
"#
    )
}

#[test]
fn committable_minimum_at_10_pow_28_loads() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        &measure_spec(&format!("minimum {COMMITTABLE_BOUND} eur")),
    );

    let now = DateTimeValue::now();
    let show = engine.show(None, "t", Some(&now)).expect("show");
    let entry = show.data.get("money").expect("data");
    assert_eq!(
        entry.lemma_type.specifications.minimum_decimal(),
        Some(decimal_lit(COMMITTABLE_BOUND))
    );

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    assert_eq!(
        json["minimum"]["value"].as_str(),
        Some(COMMITTABLE_BOUND),
        "declared measure bound must be named {{value, unit}}, got {}",
        json["minimum"]
    );
    assert_eq!(json["minimum"]["unit"].as_str(), Some("eur"));
}

#[test]
fn oversize_minimum_literal_rejected_at_parse() {
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &measure_spec(&format!("minimum {OVERSIZE_LITERAL} eur")),
    );
    assert!(
        joined.contains("Invalid number"),
        "oversize literal must fail rust_decimal parse, got: {joined}"
    );
    assert!(
        !joined.contains("cannot be represented as a decimal"),
        "must not reach planning commit check, got: {joined}"
    );
}

#[test]
fn minimum_with_milli_unit_loads_and_show_converts() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        &measure_spec_with_milli(&format!("minimum {COMMITTABLE_EUR_MINIMUM_FOR_MILLI} eur")),
    );

    let now = DateTimeValue::now();
    let show = engine.show(None, "t", Some(&now)).expect("show");
    let entry = show.data.get("money").expect("data");
    let milli = quantity_unit(&entry.lemma_type.specifications, "milli");
    assert!(
        milli.minimum_decimal().is_some(),
        "milli minimum must convert to decimal for show"
    );

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    let units = json["units"].as_array().expect("units array");
    let milli_json = units
        .iter()
        .find(|u| u["name"] == "milli")
        .expect("milli row");
    assert!(
        milli_json["minimum"].as_str().is_some(),
        "milli minimum must appear in show JSON as decimal string"
    );
}

#[test]
fn maximum_with_milli_unit_loads_and_show_converts() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        &measure_spec_with_milli(&format!("maximum {COMMITTABLE_EUR_MINIMUM_FOR_MILLI} eur")),
    );

    let now = DateTimeValue::now();
    let show = engine.show(None, "t", Some(&now)).expect("show");
    let entry = show.data.get("money").expect("data");
    let milli = quantity_unit(&entry.lemma_type.specifications, "milli");
    assert!(milli.maximum_decimal().is_some());

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    let units = json["units"].as_array().expect("units array");
    let milli_json = units
        .iter()
        .find(|u| u["name"] == "milli")
        .expect("milli row");
    assert!(milli_json["maximum"].as_str().is_some());
}

#[test]
fn suggest_with_milli_unit_loads_and_show_converts() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        &measure_spec_with_milli(&format!("suggest {COMMITTABLE_EUR_MINIMUM_FOR_MILLI} eur")),
    );

    let now = DateTimeValue::now();
    let show = engine.show(None, "t", Some(&now)).expect("show");
    let entry = show.data.get("money").expect("data");
    let milli = quantity_unit(&entry.lemma_type.specifications, "milli");
    assert!(milli.suggestion_magnitude_decimal().is_some());

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    let units = json["units"].as_array().expect("units array");
    let milli_json = units
        .iter()
        .find(|u| u["name"] == "milli")
        .expect("milli row");
    assert!(milli_json["suggestion"].as_str().is_some());
}

#[test]
fn committable_maximum_at_10_pow_28_loads() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        &measure_spec(&format!("maximum {COMMITTABLE_BOUND} eur")),
    );

    let now = DateTimeValue::now();
    let show = engine.show(None, "t", Some(&now)).expect("show");
    let entry = show.data.get("money").expect("data");
    assert_eq!(
        entry.lemma_type.specifications.maximum_decimal(),
        Some(decimal_lit(COMMITTABLE_BOUND))
    );

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    assert_eq!(
        json["maximum"]["value"].as_str(),
        Some(COMMITTABLE_BOUND),
        "declared measure bound must be named {{value, unit}}, got {}",
        json["maximum"]
    );
    assert_eq!(json["maximum"]["unit"].as_str(), Some("eur"));
}

#[test]
fn oversize_maximum_literal_rejected_at_parse() {
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &measure_spec(&format!("maximum {OVERSIZE_LITERAL} eur")),
    );
    assert!(joined.contains("Invalid number"), "got: {joined}");
    assert!(
        !joined.contains("cannot be represented as a decimal"),
        "got: {joined}"
    );
}

#[test]
fn committable_suggest_at_10_pow_28_loads() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        &measure_spec(&format!("suggest {COMMITTABLE_BOUND} eur")),
    );

    let now = DateTimeValue::now();
    let show = engine.show(None, "t", Some(&now)).expect("show");
    let entry = show.data.get("money").expect("data");
    let eur = quantity_unit(&entry.lemma_type.specifications, "eur");
    assert_eq!(
        eur.suggestion_magnitude_decimal(),
        Some(decimal_lit(COMMITTABLE_BOUND))
    );

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    let units = json["units"].as_array().expect("units array");
    let eur_json = units.iter().find(|u| u["name"] == "eur").expect("eur row");
    assert_eq!(eur_json["suggestion"].as_str(), Some(COMMITTABLE_BOUND));
}

#[test]
fn oversize_suggest_literal_rejected_at_parse() {
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &measure_spec(&format!("suggest {OVERSIZE_LITERAL} eur")),
    );
    assert!(joined.contains("Invalid number"), "got: {joined}");
    assert!(
        !joined.contains("cannot be represented as a decimal"),
        "got: {joined}"
    );
}
