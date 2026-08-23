//! Tests for assignment-shaped `->` continuations (`unit name: value`, `with path: value`).
//!
//! Run: `cargo nextest run -p lemma-engine assignment_continuation`

use super::parse;
use crate::parsing::ast::{
    CommandArg, Constraint, DataValue, LemmaSpec, TypeConstraintCommand, UnitArg, UnitFactor,
};
use crate::parsing::source::SourceType;
use crate::ResourceLimits;
use rust_decimal::Decimal;
use std::str::FromStr;

fn decimal(value: &str) -> Decimal {
    Decimal::from_str(value).expect("decimal literal in test")
}

fn parse_single_spec(source: &str) -> LemmaSpec {
    let specs = parse(source, SourceType::Volatile, &ResourceLimits::default())
        .expect("spec must parse")
        .into_flattened_specs();
    assert_eq!(specs.len(), 1, "expected one spec, got {}", specs.len());
    specs[0].clone()
}

fn measure_unit_constraints(spec: &LemmaSpec) -> Vec<Constraint> {
    let mut unit_rows = Vec::new();
    for data in &spec.data {
        let DataValue::Definition { constraints, .. } = &data.value else {
            continue;
        };
        if let Some(rows) = constraints {
            for row in rows {
                if row.command == TypeConstraintCommand::Unit {
                    unit_rows.push(row.clone());
                }
            }
        }
    }
    unit_rows
}

fn unit_name_and_arg(row: &Constraint) -> (String, UnitArg) {
    let args = &row.args;
    let name = match args.first() {
        Some(CommandArg::Label(label)) => label.clone(),
        other => panic!("unit constraint needs label name, got: {:?}", other),
    };
    let unit_arg = match args.get(1) {
        Some(CommandArg::UnitExpr(unit_arg)) => unit_arg.clone(),
        other => panic!("unit constraint needs UnitExpr payload, got: {:?}", other),
    };
    (name, unit_arg)
}

// ─── Parse: canonical colon form ───────────────────────────────────────────

#[test]
fn parse_unit_colon_factor_payload() {
    let spec = parse_single_spec(
        r#"spec money_spec
data money: measure
  -> unit eur: 1.00
"#,
    );
    let rows = measure_unit_constraints(&spec);
    assert_eq!(rows.len(), 1);
    let (name, unit_arg) = unit_name_and_arg(&rows[0]);
    assert_eq!(name, "eur");
    assert_eq!(unit_arg, UnitArg::Factor(decimal("1.00")));
    assert!(!rows[0].deprecated_without_colon);
}

#[test]
fn parse_unit_colon_compound_only_payload() {
    let spec = parse_single_spec(
        r#"spec rates
uses lemma units

data money: measure
  -> unit eur: 1.00

data rate: measure
  -> unit eur_per_hour: eur/hour
"#,
    );
    let rows = measure_unit_constraints(&spec);
    assert_eq!(rows.len(), 2);
    let (name, unit_arg) = unit_name_and_arg(&rows[1]);
    assert_eq!(name, "eur_per_hour");
    assert_eq!(
        unit_arg,
        UnitArg::Expr(
            Decimal::ONE,
            vec![
                UnitFactor {
                    measure_ref: "eur".to_string(),
                    exp: 1,
                },
                UnitFactor {
                    measure_ref: "hour".to_string(),
                    exp: -1,
                }
            ],
        )
    );
    assert!(!rows[1].deprecated_without_colon);
}

#[test]
fn parse_unit_colon_numeric_prefix_and_compound_payload() {
    let spec = parse_single_spec(
        r#"spec speed
uses lemma units

data velocity: measure
  -> unit kmh: 3.6 meter/second
"#,
    );
    let rows = measure_unit_constraints(&spec);
    assert_eq!(rows.len(), 1);
    let (name, unit_arg) = unit_name_and_arg(&rows[0]);
    assert_eq!(name, "kmh");
    assert_eq!(
        unit_arg,
        UnitArg::Expr(
            decimal("3.6"),
            vec![
                UnitFactor {
                    measure_ref: "meter".to_string(),
                    exp: 1,
                },
                UnitFactor {
                    measure_ref: "second".to_string(),
                    exp: -1,
                }
            ],
        )
    );
    assert!(!rows[0].deprecated_without_colon);
}

// ─── Parse: deprecated space-separated unit form ───────────────────────────

#[test]
fn parse_unit_deprecated_space_factor_payload() {
    let spec = parse_single_spec(
        r#"spec money_spec
data money: measure
  -> unit eur 1.00
"#,
    );
    let rows = measure_unit_constraints(&spec);
    assert_eq!(rows.len(), 1);
    let (name, unit_arg) = unit_name_and_arg(&rows[0]);
    assert_eq!(name, "eur");
    assert_eq!(unit_arg, UnitArg::Factor(decimal("1.00")));
    assert!(rows[0].deprecated_without_colon);
}

#[test]
fn parse_unit_deprecated_space_compound_only_payload() {
    let spec = parse_single_spec(
        r#"spec rates
uses lemma units

data money: measure
  -> unit eur 1.00

data rate: measure
  -> unit eur_per_hour eur/hour
"#,
    );
    let rows = measure_unit_constraints(&spec);
    assert_eq!(rows.len(), 2);
    let (name, unit_arg) = unit_name_and_arg(&rows[1]);
    assert_eq!(name, "eur_per_hour");
    assert_eq!(
        unit_arg,
        UnitArg::Expr(
            Decimal::ONE,
            vec![
                UnitFactor {
                    measure_ref: "eur".to_string(),
                    exp: 1,
                },
                UnitFactor {
                    measure_ref: "hour".to_string(),
                    exp: -1,
                }
            ],
        )
    );
    assert!(rows[1].deprecated_without_colon);
}

#[test]
fn parse_unit_deprecated_space_numeric_prefix_and_compound_payload() {
    let spec = parse_single_spec(
        r#"spec speed
uses lemma units

data velocity: measure
  -> unit kmh 3.6 meter/second
"#,
    );
    let rows = measure_unit_constraints(&spec);
    assert_eq!(rows.len(), 1);
    let (name, unit_arg) = unit_name_and_arg(&rows[0]);
    assert_eq!(name, "kmh");
    assert_eq!(
        unit_arg,
        UnitArg::Expr(
            decimal("3.6"),
            vec![
                UnitFactor {
                    measure_ref: "meter".to_string(),
                    exp: 1,
                },
                UnitFactor {
                    measure_ref: "second".to_string(),
                    exp: -1,
                }
            ],
        )
    );
    assert!(rows[0].deprecated_without_colon);
}

#[test]
fn parse_unit_deprecated_space_matches_colon_payload() {
    let canonical = parse_single_spec(
        r#"spec rates
uses lemma units

data money: measure
  -> unit eur: 1.00

data rate: measure
  -> unit eur_per_hour: eur/hour
"#,
    );
    let deprecated = parse_single_spec(
        r#"spec rates
uses lemma units

data money: measure
  -> unit eur 1.00

data rate: measure
  -> unit eur_per_hour eur/hour
"#,
    );
    let canonical_args: Vec<UnitArg> = measure_unit_constraints(&canonical)
        .iter()
        .map(|row| unit_name_and_arg(row).1)
        .collect();
    let deprecated_args: Vec<UnitArg> = measure_unit_constraints(&deprecated)
        .iter()
        .map(|row| unit_name_and_arg(row).1)
        .collect();
    assert_eq!(canonical_args, deprecated_args);
}

// ─── Parse: hard errors ────────────────────────────────────────────────────

#[test]
fn parse_unit_colon_without_payload_errors() {
    let result = parse(
        r#"spec s
data money: measure
  -> unit eur:
"#,
        SourceType::Volatile,
        &ResourceLimits::default(),
    );
    assert!(
        result.is_err(),
        "empty unit payload after colon must not parse"
    );
}

#[test]
fn parse_uses_block_with_missing_colon_errors() {
    let result = parse(
        r#"spec inner
data slot: number

spec outer
uses i: inner
  -> with slot
rule r: i.slot
"#,
        SourceType::Volatile,
        &ResourceLimits::default(),
    );
    assert!(
        result.is_err(),
        "uses block `-> with path` without colon must not parse"
    );
}

// ─── With block regression (shared assignment spine) ─────────────────────────

#[test]
fn parse_uses_block_with_colon_still_parses() {
    let specs = parse(
        r#"spec inner
data tax_rate: number

spec outer
uses i: inner
  -> with tax_rate: 0.21
rule total: i.tax_rate
"#,
        SourceType::Volatile,
        &ResourceLimits::default(),
    )
    .expect("specs must parse")
    .into_flattened_specs();
    let outer = specs
        .iter()
        .find(|spec| spec.name == "outer")
        .expect("outer spec");
    let bindings = match &outer.data[0].value {
        DataValue::Import { bindings, .. } => bindings,
        other => panic!("expected Import, got: {:?}", other),
    };
    assert_eq!(bindings.len(), 1);
    assert!(!bindings[0].deprecated_standalone_with);
    assert_eq!(bindings[0].path.name, "tax_rate");
    match &bindings[0].rhs {
        crate::parsing::ast::WithRhs::Literal(crate::parsing::ast::Value::Number(n)) => {
            assert_eq!(n, &decimal("0.21"));
        }
        other => panic!("expected numeric literal rhs, got: {:?}", other),
    }
}
