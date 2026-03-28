use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use lemma::ValueKind;
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn scale_comparison_converts_units_before_comparing() {
    let code = r#"
spec pricing
type money: scale
    -> unit eur 1
    -> unit usd 1.19

fact price: [money]

rule check: accept
    unless price > 100 usd then veto "This price is too high."
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            "pricing",
            Some(&now),
            HashMap::from([("price".to_string(), "100 eur".to_string())]),
            false,
        )
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "check")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(Some("This price is too high.".to_string()))
    );
}

#[test]
fn scale_comparison_accepts_when_conversion_makes_value_smaller() {
    let code = r#"
spec pricing
type money: scale
    -> unit eur 1
    -> unit usd 1.19

fact price: [money]

rule check: accept
    unless price > 100 usd then veto "This price is too high."
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            "pricing",
            Some(&now),
            HashMap::from([("price".to_string(), "84 eur".to_string())]),
            false,
        )
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "check")
        .unwrap();

    assert!(
        matches!(rule_result.result, OperationResult::Value(_)),
        "expected accept, got: {:?}",
        rule_result.result
    );
}

#[test]
fn scale_fact_value_requires_unit() {
    let code = r#"
spec pricing
type money: scale
    -> unit eur 1
    -> unit usd 1.19

fact price: [money]

rule check: accept
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let err = engine
        .run(
            "pricing",
            Some(&now),
            HashMap::from([("price".to_string(), "100".to_string())]),
            false,
        )
        .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("price") || msg.contains("money"),
        "actual error: {msg}"
    );
}

#[test]
fn scale_fact_value_rejects_unknown_unit() {
    let code = r#"
spec pricing
type money: scale
    -> unit eur 1
    -> unit usd 1.19

fact price: [money]

rule check: accept
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let err = engine
        .run(
            "pricing",
            Some(&now),
            HashMap::from([("price".to_string(), "100 btc".to_string())]),
            false,
        )
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("btc"), "actual error: {msg}");
}

#[test]
fn scale_in_operator_converts_units() {
    let code = r#"
spec pricing
type money: scale
    -> unit eur 1
    -> unit usd 1.19

rule price_usd: 100 eur in usd
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("pricing", Some(&now), HashMap::new(), false)
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "price_usd")
        .unwrap();

    let (value, lemma_type) = match &rule_result.result {
        OperationResult::Value(lit) => (&lit.value, &lit.lemma_type),
        other => panic!("Expected a Value result, got: {:?}", other),
    };

    assert!(
        lemma_type.is_scale(),
        "Expected scale type, got: {lemma_type:?}"
    );

    let (amount, unit) = match value {
        ValueKind::Scale(amount, unit) => (amount, unit),
        other => panic!("Expected a scale value, got: {other:?}"),
    };

    assert_eq!(*amount, Decimal::from(119));
    assert_eq!(unit.as_str(), "usd");
}

#[test]
fn scale_add_subtract_converts_units_when_same_family() {
    // Scale add/subtract with different units (same scale family) must convert, not Veto.
    // Regression: previously returned Veto "Cannot apply '-' to values with different units".
    let code = r#"
spec t
type money: scale -> unit eur 1.00 -> unit usd 1.19
fact gross: 7600 usd
fact pension: 0 eur
rule taxable: gross - pension
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine.run("t", Some(&now), HashMap::new(), false).unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "taxable")
        .unwrap();

    match &rule_result.result {
        OperationResult::Value(lit) => {
            let (amount, unit) = match &lit.value {
                ValueKind::Scale(a, u) => (a, u),
                other => panic!("expected scale, got {other:?}"),
            };
            assert_eq!(unit.as_str(), "usd", "result unit follows left operand");
            assert_eq!(*amount, Decimal::from(7600), "7600 usd - 0 eur = 7600 usd");
        }
        OperationResult::Veto(msg) => panic!("expected Value, got Veto: {msg:?}"),
    }
}

#[test]
fn scale_in_operator_rejects_unknown_unit() {
    let code = r#"
spec pricing
type money: scale
    -> unit eur 1
    -> unit usd 1.19

rule price_gbp: 100 eur in gbp
"#;

    let mut engine = Engine::new();
    let load_err = engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap_err();
    let msg = load_err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");

    assert!(msg.contains("Unknown unit 'gbp'"), "actual error: {msg}");
    assert!(msg.contains("Valid units:"), "actual error: {msg}");
}

#[test]
fn named_scale_type_comparison_with_unit_literal() {
    // Regression: planning rejected `package_weight > 1 kilogram` with
    // "Cannot compare different scale types: scale and weight" because it
    // used strict name equality instead of same_scale_family.
    let code = r#"
spec shipping

type weight: scale -> unit kilogram 1.0

fact package_weight: 2.5 kilogram

rule base_shipping: 5.99
    unless package_weight > 1 kilogram then 8.99
    unless package_weight > 5 kilogram then 15.99
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("shipping", Some(&now), HashMap::new(), false)
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "base_shipping")
        .unwrap();

    // package_weight = 2.5 kg, which is > 1 kg but not > 5 kg, so second unless wins: 8.99
    match &rule_result.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Number(d) => {
                assert_eq!(*d, Decimal::new(899, 2));
            }
            other => panic!("Expected Number value, got {:?}", other),
        },
        OperationResult::Veto(reason) => panic!("Expected value, got Veto({:?})", reason),
    }
}

#[test]
fn named_scale_type_arithmetic_within_same_family() {
    // Regression: planning rejected arithmetic between values of the same
    // scale family when their type names differed (e.g. "scale" vs "weight").
    let code = r#"
spec shipping

type money: scale -> unit USD 1.00

fact base_fee: 5.99 USD
fact surcharge: 2.00 USD

rule total: base_fee + surcharge
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("shipping", Some(&now), HashMap::new(), false)
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .unwrap();

    match &rule_result.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Scale(d, _) => {
                assert_eq!(*d, Decimal::new(799, 2));
            }
            other => panic!("Expected Scale value, got {:?}", other),
        },
        OperationResult::Veto(reason) => panic!("Expected value, got Veto({:?})", reason),
    }
}
