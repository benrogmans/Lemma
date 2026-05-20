//! Regression: stored rule results use Decimal in ValueKind and scalar JSON numbers.

use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::planning::semantics::{LiteralValue, ValueKind};
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
fn rule_number(resp: &lemma::evaluation::Response, rule: &str) -> Decimal {
    let rr = resp
        .results
        .get(rule)
        .unwrap_or_else(|| panic!("rule '{rule}' not found"));
    match &rr.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Number(d) => lemma::commit_rational_to_decimal(d).unwrap(),
            other => panic!("rule '{rule}' expected Number, got {:?}", other),
        },
        OperationResult::Veto(v) => panic!("rule '{rule}' vetoed: {v}"),
    }
}

fn assert_json_number_scalar(lit: &LiteralValue) {
    let json = serde_json::to_value(lit).expect("LiteralValue serializes");
    let number = json
        .get("value")
        .and_then(|v| v.get("number"))
        .expect("number field in JSON");
    assert!(
        number.is_string(),
        "stored number must be JSON string, got {number}"
    );
    assert!(
        !number.is_array(),
        "stored number must not serialize as [n,d], got {number}"
    );
}

#[test]
fn exact_mul_stores_decimal() {
    let code = r#"
spec s
data x: 10
rule double: x * 2
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("load");
    let now = DateTimeValue::now();
    let resp = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run");
    assert_eq!(rule_number(&resp, "double"), Decimal::from(20));
    let lit = resp.results.get("double").unwrap().result.value().unwrap();
    assert_json_number_scalar(lit);
}

#[test]
fn runtime_override_stores_decimal() {
    let code = r#"
spec s
data number_data: number
rule doubled: number_data * 2
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("load");
    let now = DateTimeValue::now();
    let resp = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::from([("number_data".to_string(), "50".to_string())]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run");
    assert_eq!(rule_number(&resp, "doubled"), Decimal::from(100));
    let lit = resp.results.get("doubled").unwrap().result.value().unwrap();
    assert_json_number_scalar(lit);
}

#[test]
fn sqrt_exact_commits_decimal() {
    let code = r#"
spec s
rule root: sqrt 9
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("load");
    let now = DateTimeValue::now();
    let resp = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run");
    assert_eq!(rule_number(&resp, "root"), Decimal::from(3));
}

#[test]
fn sqrt_irrational_stays_decimal() {
    let code = r#"
spec s
rule root: sqrt 2
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("load");
    let now = DateTimeValue::now();
    let resp = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run");
    let d = rule_number(&resp, "root");
    assert!(d > Decimal::from(1));
    assert!(d < Decimal::from(2));
    let lit = resp.results.get("root").unwrap().result.value().unwrap();
    assert!(matches!(lit.value, ValueKind::Number(_)));
    assert_json_number_scalar(lit);
}

#[test]
fn sin_no_rational_relift() {
    let code = r#"
spec s
rule s: sin 1
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("load");
    let now = DateTimeValue::now();
    let resp = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run");
    let lit = resp.results.get("s").unwrap().result.value().unwrap();
    assert!(matches!(lit.value, ValueKind::Number(_)));
    assert_json_number_scalar(lit);
}

#[test]
fn quantity_result_magnitude_decimal() {
    let code = r#"
spec s
data money: quantity
    -> unit eur 1
    -> unit usd 0.84
rule converted: 100 usd as eur
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("load");
    let now = DateTimeValue::now();
    let resp = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run");
    match &resp.results.get("converted").unwrap().result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Quantity(magnitude, unit, _) => {
                assert_eq!(*unit, "eur");
                assert_eq!(
                    lemma::commit_rational_to_decimal(magnitude).unwrap(),
                    Decimal::from(84)
                );
                let json = serde_json::to_value(lit.as_ref()).unwrap();
                let quantity_json = json.get("value").and_then(|v| v.get("quantity")).unwrap();
                assert!(
                    !quantity_json
                        .as_array()
                        .is_some_and(|a| a.len() == 2 && a[0].is_array()),
                    "quantity magnitude must not be rational array"
                );
            }
            other => panic!("expected Quantity, got {:?}", other),
        },
        OperationResult::Veto(v) => panic!("veto: {v}"),
    }
}
