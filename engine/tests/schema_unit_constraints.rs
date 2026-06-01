//! Per-unit minimum/maximum/default magnitudes on quantity and ratio schema units.

use lemma::DateTimeValue;
use lemma::Engine;
use lemma::{QuantityUnit, RatioUnit, TypeSpecification};
use rust_decimal::Decimal;
use std::str::FromStr;

fn decimal_lit(s: &str) -> Decimal {
    Decimal::from_str(s).expect("BUG: test decimal literal must parse")
}

fn load(engine: &mut Engine, code: &str, path: &str) {
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(path))),
        )
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
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "schema_unit_constraints.lemma",
            ))),
        )
        .expect_err("expected load failure");
    err.errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn qty_unit<'a>(spec: &'a TypeSpecification, name: &str) -> &'a QuantityUnit {
    match spec {
        TypeSpecification::Quantity { units, .. } => units
            .get(name)
            .unwrap_or_else(|e| panic!("unit {name}: {e}")),
        other => panic!("expected Quantity, got {other:?}"),
    }
}

fn ratio_unit<'a>(spec: &'a TypeSpecification, name: &str) -> &'a RatioUnit {
    match spec {
        TypeSpecification::Ratio { units, .. } => units
            .get(name)
            .unwrap_or_else(|e| panic!("unit {name}: {e}")),
        other => panic!("expected Ratio, got {other:?}"),
    }
}

#[test]
fn quantity_minimum_syncs_canonical_and_per_unit_magnitudes() {
    let code = r#"
spec s
data money: quantity -> unit eur 1 -> unit usd 0.91
data mass: quantity -> unit kilogram 1
data cost_per_unit: quantity
  -> unit eur_per_kilo eur/kilogram
  -> minimum 1.20 eur_per_kilo
  -> maximum 2.00 eur_per_kilo
rule out: cost_per_unit
"#;
    let mut engine = Engine::new();
    load(&mut engine, code, "qty_min.lemma");

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("cost_per_unit").expect("data");
    match &entry.lemma_type.specifications {
        TypeSpecification::Quantity {
            minimum, maximum, ..
        } => {
            let unit = qty_unit(&entry.lemma_type.specifications, "eur_per_kilo");
            assert_eq!(unit.minimum_decimal(), Some(decimal_lit("1.2")));
            assert_eq!(unit.maximum_decimal(), Some(decimal_lit("2")));
            assert_eq!(minimum.as_ref().unwrap().1, "eur_per_kilo");
            assert_eq!(maximum.as_ref().unwrap().1, "eur_per_kilo");
        }
        other => panic!("expected Quantity, got {other:?}"),
    }
}

#[test]
fn quantity_second_unit_minimum_converted_from_canonical() {
    let code = r#"
spec s
data mass: quantity
  -> unit kilogram 1
  -> unit gram 0.001
  -> minimum 1 kilogram
rule out: mass
"#;
    let mut engine = Engine::new();
    load(&mut engine, code, "qty_gram.lemma");

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("mass").expect("data");
    let gram = qty_unit(&entry.lemma_type.specifications, "gram");
    assert_eq!(gram.minimum_decimal(), Some(decimal_lit("1000")));
    let kg = qty_unit(&entry.lemma_type.specifications, "kilogram");
    assert_eq!(kg.minimum_decimal(), Some(decimal_lit("1")));
}

#[test]
fn quantity_literal_resolves_unit_index_type_with_synced_minimum() {
    let code = r#"
spec s
data money: quantity -> unit eur 1
data budget: money -> minimum 0 eur
data price: 10 eur
rule out: price
"#;
    let mut engine = Engine::new();
    load(&mut engine, code, "qty_literal_min.lemma");

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "s",
            Some(&now),
            std::collections::HashMap::new(),
            false,
        )
        .expect("eval");
    let result = response
        .results
        .values()
        .next()
        .expect("rule result")
        .display
        .clone()
        .expect("display");
    assert!(
        result.contains("10") && result.to_lowercase().contains("eur"),
        "expected 10 eur, got {result}"
    );
}

#[test]
fn ratio_schema_json_exposes_per_unit_minimum_string() {
    let code = r#"
spec s
data r: ratio
  -> unit basis_points 10000
  -> minimum 500 basis_points
rule out: r
"#;
    let mut engine = Engine::new();
    load(&mut engine, code, "ratio_bps.lemma");

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("r").expect("data");
    let unit = ratio_unit(&entry.lemma_type.specifications, "basis_points");
    assert_eq!(unit.minimum_decimal(), Some(decimal_lit("500")));

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    let units = json["units"].as_array().expect("units array");
    let bps = units
        .iter()
        .find(|u| u["name"] == "basis_points")
        .expect("basis_points row");
    assert_eq!(bps["minimum"].as_str(), Some("500"));
    assert_eq!(
        entry.lemma_type.specifications.minimum_decimal(),
        Some(decimal_lit("0.05"))
    );
}

#[test]
fn quantity_default_populates_each_unit_magnitude() {
    let code = r#"
spec s
data money: quantity
  -> unit eur 1
  -> unit usd 2
  -> default 4 eur
rule out: money
"#;
    let mut engine = Engine::new();
    load(&mut engine, code, "qty_default.lemma");

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("money").expect("data");
    let eur = qty_unit(&entry.lemma_type.specifications, "eur");
    let usd = qty_unit(&entry.lemma_type.specifications, "usd");
    assert_eq!(eur.default_magnitude_decimal(), Some(decimal_lit("4")));
    assert_eq!(usd.default_magnitude_decimal(), Some(decimal_lit("2")));
}

#[test]
fn reference_local_default_populates_per_unit_magnitudes() {
    let code = r#"
spec inner
data base: quantity
  -> unit eur 1
  -> unit usd 2

spec outer
uses i: inner
data here: i.base -> default 10 usd
rule r: here
"#;
    let mut engine = Engine::new();
    load(&mut engine, code, "ref_default.lemma");

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "outer", Some(&now)).expect("schema");
    let entry = schema.data.get("here").expect("here");
    let usd = qty_unit(&entry.lemma_type.specifications, "usd");
    let eur = qty_unit(&entry.lemma_type.specifications, "eur");
    assert_eq!(usd.default_magnitude_decimal(), Some(decimal_lit("10")));
    assert_eq!(eur.default_magnitude_decimal(), Some(decimal_lit("20")));
}

#[test]
fn precision_constraint_rejected_on_quantity_and_number() {
    for (snippet, label) in [
        (
            r#"
spec s
data x: quantity -> precision 1
rule r: x
"#,
            "quantity",
        ),
        (
            r#"
spec s
data x: number -> precision 1
rule r: x
"#,
            "number",
        ),
    ] {
        let mut engine = Engine::new();
        let msg = load_err(&mut engine, snippet).to_lowercase();
        assert!(
            msg.contains("precision") || msg.contains("unknown constraint"),
            "{label}: expected precision/unknown constraint error, got: {msg}"
        );
    }
}

#[test]
fn schema_json_round_trip_preserves_quantity_unit_bounds() {
    let code = r#"
spec s
data mass: quantity
  -> unit kilogram 1
  -> unit gram 0.001
  -> minimum 1 kilogram
rule r: mass
"#;
    let mut engine = Engine::new();
    load(&mut engine, code, "roundtrip.lemma");
    let now = DateTimeValue::now();
    let schema = engine
        .get_plan(None, "s", Some(&now))
        .expect("plan")
        .schema();
    let json = serde_json::to_string(&schema).expect("serialize");
    let round_tripped: lemma::SpecSchema = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(schema, round_tripped);
    let entry = round_tripped.data.get("mass").expect("mass");
    let gram = qty_unit(&entry.lemma_type.specifications, "gram");
    assert_eq!(gram.minimum_decimal(), Some(decimal_lit("1000")));
}

const COMPOUND_COST_PER_UNIT_SPEC: &str = r#"
spec s
data money: quantity -> unit eur 1 -> unit usd 0.91
data mass: quantity -> unit kilogram 1 -> unit tonne 1000
data cost_per_unit: quantity
  -> unit eur_per_kilo eur/kilogram
  -> unit usd_per_tonne usd/tonne
  -> maximum 2.00 eur_per_kilo
rule out: cost_per_unit
"#;

#[test]
fn compound_quantity_maximum_converts_per_unit_across_units() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        COMPOUND_COST_PER_UNIT_SPEC,
        "compound_max.lemma",
    );

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("cost_per_unit").expect("data");

    let eur_per_kilo = qty_unit(&entry.lemma_type.specifications, "eur_per_kilo");
    let usd_per_tonne = qty_unit(&entry.lemma_type.specifications, "usd_per_tonne");

    assert_eq!(eur_per_kilo.maximum_decimal(), Some(decimal_lit("2")));
    assert_ne!(
        usd_per_tonne.maximum_decimal(),
        Some(decimal_lit("2")),
        "usd_per_tonne maximum must be converted from eur_per_kilo bound, not copied as 2"
    );

    assert_eq!(
        usd_per_tonne.maximum_canonical_decimal(),
        eur_per_kilo.maximum_canonical_decimal(),
        "per-unit maxima must represent the same canonical bound"
    );

    let json = serde_json::to_value(&entry.lemma_type).expect("serde");
    let units = json["units"].as_array().expect("units array");
    let usd_json = units
        .iter()
        .find(|u| u["name"] == "usd_per_tonne")
        .expect("usd_per_tonne row");
    assert_ne!(
        usd_json["maximum"].as_str(),
        Some("2"),
        "schema JSON must not expose stale usd_per_tonne maximum"
    );
}

#[test]
fn compound_quantity_maximum_in_bound_unit_stays_literal() {
    let code = r#"
spec s
data money: quantity -> unit eur 1 -> unit usd 0.91
data mass: quantity -> unit kilogram 1 -> unit tonne 1000
data cost_per_unit: quantity
  -> unit eur_per_kilo eur/kilogram
  -> unit usd_per_tonne usd/tonne
  -> maximum 2.00 usd_per_tonne
rule out: cost_per_unit
"#;
    let mut engine = Engine::new();
    load(&mut engine, code, "compound_max_bound_unit.lemma");

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("cost_per_unit").expect("data");

    let usd_per_tonne = qty_unit(&entry.lemma_type.specifications, "usd_per_tonne");
    assert_eq!(
        usd_per_tonne.maximum_decimal(),
        Some(decimal_lit("2")),
        "maximum declared in usd_per_tonne must stay 2 in that unit"
    );
}

const TRI_COMPOUND_COST_SPEC: &str = r#"
spec s
uses lemma units

data money: quantity
  -> unit eur 1
  -> unit usd 0.91

data mass: quantity
  -> unit kilogram 1
  -> unit tonne 1000

data storage_cost: quantity
  -> unit eur_per_kilo_hour eur/kilogram/hour
  -> unit usd_per_ton_hour usd/tonne/hour
  -> maximum 2.00 eur_per_kilo_hour

rule out: storage_cost
"#;

#[test]
fn tri_compound_quantity_maximum_converts_per_unit_across_units() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        TRI_COMPOUND_COST_SPEC,
        "tri_compound_max.lemma",
    );

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("storage_cost").expect("data");

    let eur_per_kilo_hour = qty_unit(&entry.lemma_type.specifications, "eur_per_kilo_hour");
    let usd_per_ton_hour = qty_unit(&entry.lemma_type.specifications, "usd_per_ton_hour");

    assert_eq!(eur_per_kilo_hour.maximum_decimal(), Some(decimal_lit("2")));
    assert_ne!(
        usd_per_ton_hour.maximum_decimal(),
        Some(decimal_lit("2")),
        "usd_per_ton_hour maximum must be converted from eur_per_kilo_hour bound, not copied as 2"
    );

    assert_eq!(
        usd_per_ton_hour.maximum_canonical_decimal(),
        eur_per_kilo_hour.maximum_canonical_decimal(),
        "per-unit maxima must represent the same canonical bound across three referenced quantities"
    );
}
