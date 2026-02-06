use lemma::evaluation::OperationResult;
use lemma::Engine;
use lemma::ValueKind;
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn scale_comparison_converts_units_before_comparing() {
    let code = r#"
doc pricing
type money = scale
    -> unit eur 1
    -> unit usd 1.19

fact price = [money]

rule check = accept
    unless price > 100 usd then veto "This price is too high."
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine
        .evaluate(
            "pricing",
            vec![],
            HashMap::from([("price".to_string(), "100 eur".to_string())]),
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
doc pricing
type money = scale
    -> unit eur 1
    -> unit usd 1.19

fact price = [money]

rule check = accept
    unless price > 100 usd then veto "This price is too high."
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine
        .evaluate(
            "pricing",
            vec![],
            HashMap::from([("price".to_string(), "84 eur".to_string())]),
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
doc pricing
type money = scale
    -> unit eur 1
    -> unit usd 1.19

fact price = [money]

rule check = accept
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let err = engine
        .evaluate(
            "pricing",
            vec![],
            HashMap::from([("price".to_string(), "100".to_string())]),
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
doc pricing
type money = scale
    -> unit eur 1
    -> unit usd 1.19

fact price = [money]

rule check = accept
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let err = engine
        .evaluate(
            "pricing",
            vec![],
            HashMap::from([("price".to_string(), "100 btc".to_string())]),
        )
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("btc"), "actual error: {msg}");
}

#[test]
fn scale_in_operator_converts_units() {
    let code = r#"
doc pricing
type money = scale
    -> unit eur 1
    -> unit usd 1.19

rule price_usd = 100 eur in usd
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("pricing", vec![], HashMap::new()).unwrap();
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
fn scale_in_operator_rejects_unknown_unit() {
    let code = r#"
doc pricing
type money = scale
    -> unit eur 1
    -> unit usd 1.19

rule price_gbp = 100 eur in gbp
"#;

    let mut engine = Engine::new();
    let err = engine.add_lemma_code(code, "test.lemma").unwrap_err();
    let msg = err.to_string();

    assert!(msg.contains("Unknown unit 'gbp'"), "actual error: {msg}");
    assert!(msg.contains("Valid units:"), "actual error: {msg}");
}
