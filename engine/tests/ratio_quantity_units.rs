//! Rock-solid tests locking in ratio vs quantity unit behaviour.
//!
//! Covers: "as percent" / "as permille" ratio conversion; comparison with percent literals;
//! unknown unit error; quantity conversion unchanged; ratio display with no unit;
//! number ± ratio data semantics (e.g. 100 - discount: 100 * (1 - discount)).

use lemma::DateTimeValue;
use lemma::ValueKind;
use lemma::{Engine, LiteralValue};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn decimal_lit(s: &str) -> Decimal {
    Decimal::from_str(s).expect("BUG: test decimal literal must parse")
}

fn rule_literal(rule: &lemma::RuleResult) -> &lemma::LiteralValue {
    assert!(!rule.vetoed, "unexpected veto: {:?}", rule.veto_reason);
    rule.explanation
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value")
}

#[test]
fn in_percent_produces_ratio_and_compares_with_percent_literal() {
    let code = r#"
spec savings
data savings_amount: 75
data total_amount: 300

rule savings_ratio: (savings_amount / total_amount) as percent
rule is_above_20: savings_ratio > 20%
rule is_above_30: savings_ratio > 30%
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "savings", Some(&now), HashMap::new(), true, None)
        .unwrap();

    let ratio_result = response
        .results
        .get("savings_ratio")
        .expect("savings_ratio rule");
    let lit = rule_literal(ratio_result);
    match &lit.value {
        ValueKind::Ratio(r, u) => {
            assert_eq!(
                lemma::ValueKind::Number(r.clone())
                    .as_decimal_magnitude()
                    .unwrap(),
                Decimal::new(25, 2),
                "75/300 = 0.25"
            );
            assert_eq!(u.as_deref(), Some("percent"));
        }
        other => panic!("savings_ratio must be Ratio, got {:?}", other),
    }

    let above_20 = response.results.get("is_above_20").expect("is_above_20");
    let above_30 = response.results.get("is_above_30").expect("is_above_30");
    assert_eq!(above_20.boolean, Some(true));
    assert_eq!(above_30.boolean, Some(false));
}

#[test]
fn in_percent_then_chained_comparison_with_multiple_thresholds() {
    let code = r#"
spec summary
data part: 18
data whole: 60

rule pct: (part / whole) as percent
rule tier: "low"
    unless pct > 25% then "mid"
    unless pct > 50% then "high"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "summary", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let tier = response.results.get("tier").expect("tier");
    assert_eq!(tier.text.as_deref(), Some("mid"));
}

#[test]
fn in_permille_produces_ratio() {
    let code = r#"
spec permille_spec
data value: 0.025

rule as_permille: value as permille
rule above_20_permille: as_permille > 20 permille
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "permille_spec",
            Some(&now),
            HashMap::new(),
            true,
            None,
        )
        .unwrap();
    let as_permille = response.results.get("as_permille").expect("as_permille");
    let lit = rule_literal(as_permille);
    match &lit.value {
        ValueKind::Ratio(r, u) => {
            assert_eq!(
                lemma::ValueKind::Number(r.clone())
                    .as_decimal_magnitude()
                    .unwrap(),
                Decimal::new(25, 3)
            );
            assert_eq!(u.as_deref(), Some("permille"));
        }
        other => panic!("as_permille must be Ratio, got {:?}", other),
    }

    let above = response.results.get("above_20_permille").expect("above");
    assert_eq!(above.boolean, Some(true));
}

#[test]
fn unknown_unit_in_conversion_fails_planning() {
    let code = r#"
spec bad
data x: 100

rule bad_conv: x as not_a_unit
"#;

    let mut engine = Engine::new();
    let err = engine.load(code, lemma::SourceType::Volatile).unwrap_err();

    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Unknown unit") && msg.contains("not_a_unit"),
        "expected unknown unit error, got: {}",
        msg
    );
}

#[test]
fn number_minus_ratio_data_is_100_times_one_minus_discount() {
    let code = r#"
spec pricing
data discount: ratio

rule price: 100 - discount
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::from([("discount".to_string(), "20 percent".to_string())]),
            true,
            None,
        )
        .unwrap();

    let price = response.results.get("price").expect("price rule");
    let lit = rule_literal(price);
    if let ValueKind::Number(n) = &lit.value {
        assert_eq!(
            lemma::ValueKind::Number(n.clone())
                .as_decimal_magnitude()
                .unwrap(),
            Decimal::from(80),
            "100 - 20% = 100 * (1 - 0.20) = 80"
        );
    } else {
        panic!("price should be Number, got {:?}", lit.value);
    }
}

#[test]
fn ratio_display_with_none_unit_shows_number_only() {
    let lit = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    let display = lit.display_value();
    assert!(
        !display.contains("percent") && display.contains("0.5"),
        "ratio with None unit should display number only, got: {}",
        display
    );

    let with_unit =
        LiteralValue::ratio_from_decimal(decimal_lit("0.5"), Some("percent".to_string()));
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
data a: 10
data b: 40

rule pct: (a / b) as percent
rule plus_five: pct + 5%
rule compared: plus_five > 25%
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "chained", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let pct = response.results.get("pct").expect("pct");
    let plus_five = response.results.get("plus_five").expect("plus_five");
    let compared = response.results.get("compared").expect("compared");

    if let ValueKind::Ratio(r, _) = &rule_literal(pct).value {
        assert_eq!(
            lemma::ValueKind::Number(r.clone())
                .as_decimal_magnitude()
                .unwrap(),
            Decimal::new(25, 2)
        );
    } else {
        panic!("pct must be Ratio");
    }
    if let ValueKind::Ratio(r, _) = &rule_literal(plus_five).value {
        assert_eq!(
            lemma::ValueKind::Number(r.clone())
                .as_decimal_magnitude()
                .unwrap(),
            Decimal::new(30, 2)
        );
    } else {
        panic!("plus_five must be Ratio");
    }
    assert_eq!(compared.boolean, Some(true));
}

#[test]
fn quantity_and_ratio_conversion_in_same_spec() {
    let code = r#"
spec mixed
data money: quantity
  -> unit eur 1

data amount: 200
data part: 50

rule as_eur: amount as eur
rule share_pct: (part / amount) as percent
rule share_above_20: share_pct > 20%
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "mixed", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let as_eur = response.results.get("as_eur").expect("as_eur");
    let share_pct = response.results.get("share_pct").expect("share_pct");
    let share_above_20 = response
        .results
        .get("share_above_20")
        .expect("share_above_20");

    let lit = rule_literal(as_eur);
    match &lit.value {
        ValueKind::Quantity(n, signature) => {
            assert_eq!(
                ValueKind::Number(n.clone()).as_decimal_magnitude().unwrap(),
                Decimal::from(200)
            );
            assert_eq!(
                signature.first().map(|(name, _)| name.as_str()),
                Some("eur")
            );
        }
        other => panic!("as_eur: expected Quantity, got {other:?}"),
    }
    let lit = rule_literal(share_pct);
    if let ValueKind::Ratio(r, u) = &lit.value {
        assert_eq!(
            lemma::ValueKind::Number(r.clone())
                .as_decimal_magnitude()
                .unwrap(),
            Decimal::new(25, 2)
        );
        assert_eq!(u.as_deref(), Some("percent"));
    } else {
        panic!("share_pct must be Ratio");
    }
    assert_eq!(share_above_20.boolean, Some(true));
}
