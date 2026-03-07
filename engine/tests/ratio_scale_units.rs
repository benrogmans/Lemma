//! Rock-solid tests locking in ratio vs scale unit behaviour.
//!
//! Covers: "in percent" / "in permille" as ratio conversion; comparison with percent literals;
//! unknown unit error; scale conversion unchanged; ratio display with no unit;
//! number ± ratio fact semantics (e.g. 100 - discount: 100 * (1 - discount)).

use lemma::evaluation::OperationResult;
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use lemma::planning::semantics::ValueKind;
use lemma::{Engine, LiteralValue};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn in_percent_produces_ratio_and_compares_with_percent_literal() {
    let code = r#"
spec savings
fact savings_amount: 75
fact total_amount: 300

rule savings_ratio: (savings_amount / total_amount) in percent
rule is_above_20: savings_ratio > 20%
rule is_above_30: savings_ratio > 30%
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("savings", None, &now, vec![], HashMap::new())
        .unwrap();

    let ratio_result = response
        .results
        .get("savings_ratio")
        .expect("savings_ratio rule");
    match &ratio_result.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Ratio(r, u) => {
                assert_eq!(*r, Decimal::new(25, 2), "75/300 = 0.25");
                assert_eq!(u.as_deref(), Some("percent"));
            }
            _ => panic!("savings_ratio must be Ratio, got {:?}", lit.value),
        },
        other => panic!("savings_ratio must be Value, got {:?}", other),
    }

    let above_20 = response.results.get("is_above_20").expect("is_above_20");
    let above_30 = response.results.get("is_above_30").expect("is_above_30");
    match (&above_20.result, &above_30.result) {
        (OperationResult::Value(a), OperationResult::Value(b)) => {
            assert!(matches!(&a.value, ValueKind::Boolean(true)), "25% > 20%");
            assert!(
                matches!(&b.value, ValueKind::Boolean(false)),
                "25% not > 30%"
            );
        }
        _ => panic!("comparison rules must yield Value(bool)"),
    }
}

#[test]
fn in_percent_then_chained_comparison_with_multiple_thresholds() {
    let code = r#"
spec summary
fact part: 18
fact whole: 60

rule pct: (part / whole) in percent
rule tier: "low"
    unless pct > 25% then "mid"
    unless pct > 50% then "high"
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("summary", None, &now, vec![], HashMap::new())
        .unwrap();
    let tier = response.results.get("tier").expect("tier");
    match &tier.result {
        OperationResult::Value(lit) => {
            assert!(matches!(&lit.value, ValueKind::Text(s) if s == "mid"));
        }
        _ => panic!("tier should be Value"),
    }
}

#[test]
fn in_permille_produces_ratio() {
    let code = r#"
spec permille_spec
fact value: 0.025

rule as_permille: value in permille
rule above_20_permille: as_permille > 20 permille
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("permille_spec", None, &now, vec![], HashMap::new())
        .unwrap();
    let as_permille = response.results.get("as_permille").expect("as_permille");
    match &as_permille.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Ratio(r, u) => {
                assert_eq!(*r, Decimal::new(25, 3));
                assert_eq!(u.as_deref(), Some("permille"));
            }
            _ => panic!("as_permille must be Ratio, got {:?}", lit.value),
        },
        _ => panic!("as_permille must be Value"),
    }

    let above = response.results.get("above_20_permille").expect("above");
    match &above.result {
        OperationResult::Value(lit) => assert!(matches!(&lit.value, ValueKind::Boolean(true))),
        _ => panic!("above_20_permille must be Value(bool)"),
    }
}

#[test]
fn unknown_unit_in_conversion_fails_planning() {
    let code = r#"
spec bad
fact x: 100

rule bad_conv: x in not_a_unit
"#;

    let mut engine = Engine::new();
    let err = add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap_err();

    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Unknown unit") && msg.contains("not_a_unit"),
        "expected unknown unit error, got: {}",
        msg
    );
}

#[test]
fn scale_in_eur_still_works_unchanged() {
    let code = r#"
spec pricing
type money: scale
  -> unit eur 1
  -> unit usd 1.1

fact amount: 100

rule in_eur: amount in eur
rule in_usd: amount in usd
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("pricing", None, &now, vec![], HashMap::new())
        .unwrap();
    let in_eur = response.results.get("in_eur").expect("in_eur");
    let in_usd = response.results.get("in_usd").expect("in_usd");

    match (&in_eur.result, &in_usd.result) {
        (OperationResult::Value(e), OperationResult::Value(u)) => {
            assert!(matches!(&e.value, ValueKind::Scale(_, unit) if unit == "eur"));
            assert!(matches!(&u.value, ValueKind::Scale(_, unit) if unit == "usd"));
        }
        _ => panic!("scale conversions must succeed"),
    }
}

#[test]
fn number_minus_ratio_fact_is_100_times_one_minus_discount() {
    let code = r#"
spec pricing
fact discount: [ratio]

rule price: 100 - discount
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate(
            "pricing",
            None,
            &now,
            vec![],
            HashMap::from([("discount".to_string(), "20 percent".to_string())]),
        )
        .unwrap();

    let price = response.results.get("price").expect("price rule");
    match &price.result {
        OperationResult::Value(lit) => {
            if let ValueKind::Number(n) = &lit.value {
                assert_eq!(*n, Decimal::from(80), "100 - 20% = 100 * (1 - 0.20) = 80");
            } else {
                panic!("price should be Number, got {:?}", lit.value);
            }
        }
        _ => panic!("price should be Value"),
    }
}

#[test]
fn ratio_display_with_none_unit_shows_number_only() {
    let lit = LiteralValue::ratio(Decimal::from_str("0.5").unwrap(), None);
    let display = lit.display_value();
    assert!(
        !display.contains("percent") && display.contains("0.5"),
        "ratio with None unit should display number only, got: {}",
        display
    );

    let with_unit = LiteralValue::ratio(
        Decimal::from_str("0.5").unwrap(),
        Some("percent".to_string()),
    );
    let display_with = with_unit.display_value();
    assert!(
        display_with.contains('%'),
        "ratio with Some(percent) should show % symbol, got: {}",
        display_with
    );
}

#[test]
fn chained_ratio_conversion_and_arithmetic() {
    let code = r#"
spec chained
fact a: 10
fact b: 40

rule pct: (a / b) in percent
rule plus_five: pct + 5%
rule compared: plus_five > 25%
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("chained", None, &now, vec![], HashMap::new())
        .unwrap();
    let pct = response.results.get("pct").expect("pct");
    let plus_five = response.results.get("plus_five").expect("plus_five");
    let compared = response.results.get("compared").expect("compared");

    match &pct.result {
        OperationResult::Value(lit) => {
            if let ValueKind::Ratio(r, _) = &lit.value {
                assert_eq!(*r, Decimal::new(25, 2));
            }
        }
        _ => panic!("pct must be Value"),
    }
    match &plus_five.result {
        OperationResult::Value(lit) => {
            if let ValueKind::Ratio(r, _) = &lit.value {
                assert_eq!(*r, Decimal::new(30, 2));
            }
        }
        _ => panic!("plus_five must be Value"),
    }
    match &compared.result {
        OperationResult::Value(lit) => assert!(matches!(&lit.value, ValueKind::Boolean(true))),
        _ => panic!("compared must be Value(bool)"),
    }
}

#[test]
fn scale_and_ratio_conversion_in_same_spec() {
    let code = r#"
spec mixed
type money: scale
  -> unit eur 1

fact amount: 200
fact part: 50

rule as_eur: amount in eur
rule share_pct: (part / amount) in percent
rule share_above_20: share_pct > 20%
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("mixed", None, &now, vec![], HashMap::new())
        .unwrap();
    let as_eur = response.results.get("as_eur").expect("as_eur");
    let share_pct = response.results.get("share_pct").expect("share_pct");
    let share_above_20 = response
        .results
        .get("share_above_20")
        .expect("share_above_20");

    match &as_eur.result {
        OperationResult::Value(lit) => {
            assert!(
                matches!(&lit.value, ValueKind::Scale(n, u) if *n == Decimal::from(200) && u == "eur"),
                "as_eur: got {:?}",
                lit.value
            );
        }
        _ => panic!("as_eur must be Scale"),
    }
    match &share_pct.result {
        OperationResult::Value(lit) => {
            if let ValueKind::Ratio(r, u) = &lit.value {
                assert_eq!(*r, Decimal::new(25, 2));
                assert_eq!(u.as_deref(), Some("percent"));
            }
        }
        _ => panic!("share_pct must be Ratio"),
    }
    match &share_above_20.result {
        OperationResult::Value(lit) => assert!(matches!(&lit.value, ValueKind::Boolean(true))),
        _ => panic!("share_above_20 must be Value(bool)"),
    }
}

#[test]
fn ratio_comparison_both_sides_ratio() {
    let code = r#"
spec compare
fact discount: 15%
fact threshold: 10%

rule meets: discount >= threshold
rule exceeds: discount > threshold
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("compare", None, &now, vec![], HashMap::new())
        .unwrap();
    let meets = response.results.get("meets").expect("meets");
    let exceeds = response.results.get("exceeds").expect("exceeds");

    match (&meets.result, &exceeds.result) {
        (OperationResult::Value(m), OperationResult::Value(e)) => {
            assert!(matches!(&m.value, ValueKind::Boolean(true)));
            assert!(matches!(&e.value, ValueKind::Boolean(true)));
        }
        _ => panic!("comparison rules must yield Value"),
    }
}
