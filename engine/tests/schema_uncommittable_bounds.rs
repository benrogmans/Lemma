//! Declared quantity bounds follow `rust_decimal` for input and schema output.
//!
//! IN: literals in `.lemma` must parse as `Decimal`. Oversize literals are rejected at parse.
//! OUT: rationals stored after planning must commit to `Decimal` for schema export, or planning
//! rejects with `cannot be represented as a decimal`.

use lemma::DateTimeValue;
use lemma::Engine;
use lemma::TypeSpecification;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Largest power of ten that parses as a `Decimal` and commits through planning/schema.
const COMMITTABLE_BOUND: &str = "10000000000000000000000000000";

/// First power of ten above the `Decimal` literal range; rejected at parse, not planning.
const OVERSIZE_LITERAL: &str = "100000000000000000000000000000";

fn decimal_lit(s: &str) -> Decimal {
    Decimal::from_str(s).expect("BUG: test decimal literal must parse")
}

fn load(engine: &mut Engine, code: &str) {
    engine
        .load(code, lemma::SourceType::Volatile)
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
        .load(code, lemma::SourceType::Volatile)
        .expect_err("expected load failure");
    err.errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn qty_unit<'a>(spec: &'a TypeSpecification, name: &str) -> &'a lemma::QuantityUnit {
    match spec {
        TypeSpecification::Quantity { units, .. } => units
            .get(name)
            .expect("BUG: quantity unit must exist in test fixture"),
        _ => panic!("BUG: test fixture must use quantity type"),
    }
}

fn quantity_spec(constraint: &str) -> String {
    format!(
        r#"
spec t
data money: quantity
  -> unit eur 1
  -> {constraint}
rule r: money
"#
    )
}

fn quantity_spec_with_milli(constraint: &str) -> String {
    format!(
        r#"
spec t
data money: quantity
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
        &quantity_spec(&format!("minimum {COMMITTABLE_BOUND} eur")),
    );

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "t", Some(&now)).expect("schema");
    let entry = schema.data.get("money").expect("data");
    assert_eq!(
        entry.lemma_type.specifications.minimum_decimal(),
        Some(decimal_lit(COMMITTABLE_BOUND))
    );

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    assert_eq!(
        json["minimum"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_str()),
        Some(COMMITTABLE_BOUND)
    );
}

#[test]
fn oversize_minimum_literal_rejected_at_parse() {
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &quantity_spec(&format!("minimum {OVERSIZE_LITERAL} eur")),
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
fn uncommittable_minimum_per_unit_magnitude_rejected_at_planning() {
    // IN: 10^28 eur parses. OUT: milli per-unit magnitude (10^31 in Q) cannot commit to Decimal.
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &quantity_spec_with_milli(&format!("minimum {COMMITTABLE_BOUND} eur")),
    );
    assert!(
        joined.contains("cannot be represented as a decimal"),
        "got: {joined}"
    );
    assert!(joined.contains("unit 'milli' minimum"), "got: {joined}");
}

#[test]
fn committable_maximum_at_10_pow_28_loads() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        &quantity_spec(&format!("maximum {COMMITTABLE_BOUND} eur")),
    );

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "t", Some(&now)).expect("schema");
    let entry = schema.data.get("money").expect("data");
    assert_eq!(
        entry.lemma_type.specifications.maximum_decimal(),
        Some(decimal_lit(COMMITTABLE_BOUND))
    );

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    assert_eq!(
        json["maximum"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_str()),
        Some(COMMITTABLE_BOUND)
    );
}

#[test]
fn oversize_maximum_literal_rejected_at_parse() {
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &quantity_spec(&format!("maximum {OVERSIZE_LITERAL} eur")),
    );
    assert!(joined.contains("Invalid number"), "got: {joined}");
    assert!(
        !joined.contains("cannot be represented as a decimal"),
        "got: {joined}"
    );
}

#[test]
fn uncommittable_maximum_per_unit_magnitude_rejected_at_planning() {
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &quantity_spec_with_milli(&format!("maximum {COMMITTABLE_BOUND} eur")),
    );
    assert!(
        joined.contains("cannot be represented as a decimal"),
        "got: {joined}"
    );
    assert!(joined.contains("unit 'milli' maximum"), "got: {joined}");
}

#[test]
fn committable_default_at_10_pow_28_loads() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        &quantity_spec(&format!("default {COMMITTABLE_BOUND} eur")),
    );

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "t", Some(&now)).expect("schema");
    let entry = schema.data.get("money").expect("data");
    let eur = qty_unit(&entry.lemma_type.specifications, "eur");
    assert_eq!(
        eur.default_magnitude_decimal(),
        Some(decimal_lit(COMMITTABLE_BOUND))
    );

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    let units = json["units"].as_array().expect("units array");
    let eur_json = units.iter().find(|u| u["name"] == "eur").expect("eur row");
    assert_eq!(eur_json["default"].as_str(), Some(COMMITTABLE_BOUND));
}

#[test]
fn oversize_default_literal_rejected_at_parse() {
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &quantity_spec(&format!("default {OVERSIZE_LITERAL} eur")),
    );
    assert!(joined.contains("Invalid number"), "got: {joined}");
    assert!(
        !joined.contains("cannot be represented as a decimal"),
        "got: {joined}"
    );
}

#[test]
fn uncommittable_default_per_unit_magnitude_rejected_at_planning() {
    let mut engine = Engine::new();
    let joined = load_err(
        &mut engine,
        &quantity_spec_with_milli(&format!("default {COMMITTABLE_BOUND} eur")),
    );
    assert!(
        joined.contains("cannot be represented as a decimal"),
        "got: {joined}"
    );
    assert!(joined.contains("unit 'milli' default"), "got: {joined}");
}
