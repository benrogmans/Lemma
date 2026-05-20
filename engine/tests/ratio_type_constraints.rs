//! Ratio typedef constraints (`minimum`, `maximum`, `default`) with custom units.
//!
//! Parser emits `N label` as `Value::NumberWithUnit`; planning must resolve against
//! the typedef's `RatioUnits` (same as quantity constraints after scale rename).

use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::planning::semantics::TypeSpecification;
use lemma::Engine;
use lemma::ValueKind;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn decimal_lit(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}
fn rational_lit(d: &str) -> lemma::RationalInteger {
    lemma::decimal_to_rational(decimal_lit(d)).unwrap()
}

fn load(engine: &mut Engine, code: &str) {
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "ratio_constraints.lemma",
            ))),
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

fn bps_spec() -> &'static str {
    r#"
spec s
data r: ratio
  -> unit basis_points 10000
  -> minimum 500 basis_points
  -> maximum 10000 basis_points
rule out: r
"#
}

#[test]
fn ratio_minimum_custom_unit_loads_with_canonical_bounds() {
    let mut engine = Engine::new();
    load(&mut engine, bps_spec());

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("r").expect("data r");
    match &entry.lemma_type.specifications {
        TypeSpecification::Ratio {
            minimum,
            maximum,
            units,
            ..
        } => {
            assert_eq!(
                *minimum,
                Some(rational_lit("0.05")),
                "500 basis_points / 10000"
            );
            assert_eq!(
                *maximum,
                Some(rational_lit("1")),
                "10000 basis_points / 10000"
            );
            let bps = units.get("basis_points").expect("basis_points unit");
            assert_eq!(bps.minimum, Some(rational_lit("500")));
            assert_eq!(bps.maximum, Some(rational_lit("10000")));
        }
        other => panic!("expected Ratio type, got {:?}", other),
    }
}

#[test]
fn ratio_bare_minimum_rejected_at_load() {
    let code = r#"
spec s
data r: ratio -> minimum 0
rule out: r
"#;
    let mut engine = Engine::new();
    let err = engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "ratio_constraints.lemma",
            ))),
        )
        .expect_err("bare number minimum on ratio must fail");
    let s = err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    assert!(
        s.contains("ratio") && (s.contains("unit") || s.contains("%") || s.contains("bare")),
        "expected ratio unit syntax error, got: {s}"
    );
}

#[test]
fn ratio_minimum_percent_constraint_loads() {
    let code = r#"
spec s
data r: ratio -> minimum 10% -> maximum 100%
rule out: r
"#;
    let mut engine = Engine::new();
    load(&mut engine, code);

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("r").expect("data r");
    match &entry.lemma_type.specifications {
        TypeSpecification::Ratio { minimum, .. } => {
            assert_eq!(*minimum, Some(rational_lit("0.10")));
        }
        other => panic!("expected Ratio type, got {:?}", other),
    }
}

#[test]
fn ratio_minimum_custom_unit_override_enforced() {
    let mut engine = Engine::new();
    load(&mut engine, bps_spec());

    let mut data = HashMap::new();
    data.insert("r".to_string(), "400 basis_points".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run(
            None,
            "s",
            Some(&now),
            data,
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect_err("400 bps < 500 bps minimum");
    let message = err.to_string();
    assert!(
        message.contains("minimum") || message.to_lowercase().contains("below"),
        "expected minimum violation, got: {message}"
    );
    assert!(
        message.contains("500 basis_points"),
        "expected per-unit bound in message, got: {message}"
    );
    assert!(
        !message.contains("0.05"),
        "must not show canonical ratio magnitude, got: {message}"
    );
    assert!(
        !message.to_lowercase().contains("canonical"),
        "must not mention canonical units, got: {message}"
    );
}

#[test]
fn ratio_default_custom_unit_loads() {
    let code = r#"
spec s
data r: ratio
  -> unit basis_points 10000
  -> default 500 basis_points
rule out: r
"#;
    let mut engine = Engine::new();
    load(&mut engine, code);

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "s", Some(&now)).expect("schema");
    let entry = schema.data.get("r").expect("data r");
    let default = entry.default.as_ref().expect("declared default");
    match &default.value {
        ValueKind::Ratio(n, u) => {
            assert_eq!(
                lemma::commit_rational_to_decimal(n).unwrap(),
                decimal_lit("0.05")
            );
            assert_eq!(u.as_deref(), Some("basis_points"));
        }
        other => panic!("expected Ratio default, got {:?}", other),
    }
}

#[test]
fn ratio_minimum_custom_unit_override_accepts_at_bound() {
    let mut engine = Engine::new();
    load(&mut engine, bps_spec());

    let mut data = HashMap::new();
    data.insert("r".to_string(), "500 basis_points".to_string());

    let now = DateTimeValue::now();
    let resp = engine
        .run(
            None,
            "s",
            Some(&now),
            data,
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("500 bps meets minimum");
    let rr = resp.results.get("out").expect("rule out");
    match &rr.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Ratio(n, u) => {
                assert_eq!(
                    lemma::commit_rational_to_decimal(n).unwrap(),
                    decimal_lit("0.05")
                );
                assert_eq!(u.as_deref(), Some("basis_points"));
            }
            other => panic!("expected Ratio, got {:?}", other),
        },
        OperationResult::Veto(v) => panic!("unexpected veto: {v}"),
    }
}
