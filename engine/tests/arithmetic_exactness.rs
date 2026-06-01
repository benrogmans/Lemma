//! Arithmetic and division behavior (planning errors vs runtime Veto vs Decimal results).

use lemma::{Engine, LiteralValue, ValueKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("arithmetic_exactness.lemma")))
}

fn eval_quantity_rule(code: &str, spec_name: &str, rule_name: &str) -> LiteralValue {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("spec must load");
    let response = engine
        .run(None, spec_name, None, HashMap::new(), true)
        .expect("spec must evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("rule '{}' missing", rule_name))
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .unwrap_or_else(|| panic!("rule '{}' must return a value", rule_name))
        .clone()
}

#[test]
fn literal_division_by_zero_is_planning_error() {
    let code = r#"
        spec exactness
        rule quotient: 1 / 0
    "#;
    let mut engine = Engine::new();
    let errors = engine
        .load(code, source())
        .expect_err("literal zero divisor must not load");
    let message = format!("{:?}", errors);
    assert!(
        message.to_lowercase().contains("zero"),
        "expected planning error mentioning zero divisor, got: {}",
        message
    );
}

#[test]
fn runtime_data_zero_divisor_still_evaluates_to_veto() {
    let code = r#"
        spec exactness
        data denominator: 0
        rule quotient: 10 / denominator
    "#;
    let mut engine = Engine::new();
    engine
        .load(code, source())
        .expect("runtime zero divisor must load");
    let response = engine
        .run(None, "exactness", None, HashMap::new(), true)
        .expect("evaluation must complete");
    let rule_result = response
        .results
        .get("quotient")
        .expect("quotient rule missing");
    assert!(rule_result.vetoed);
    assert!(
        matches!(
            rule_result.veto_detail,
            Some(lemma::VetoType::Computation { .. })
        ),
        "runtime zero divisor must veto, not fail load"
    );
}

#[test]
fn runtime_data_ten_divide_three_returns_value_not_veto() {
    let code = r#"
        spec exactness
        data ten: 10
        data three: 3
        rule quotient: ten / three
    "#;
    let mut engine = Engine::new();
    engine
        .load(code, source())
        .expect("runtime division must load");
    let response = engine
        .run(None, "exactness", None, HashMap::new(), true)
        .expect("evaluation must complete");
    let rule_result = response
        .results
        .get("quotient")
        .expect("quotient rule missing");
    let value = rule_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("ten / three must return Value, not Veto");
    match &value.value {
        ValueKind::Number(n) => {
            assert!(
                lemma::ValueKind::Number(*n).as_decimal_magnitude().unwrap()
                    > rust_decimal::Decimal::from(3)
                    && lemma::ValueKind::Number(*n).as_decimal_magnitude().unwrap()
                        < rust_decimal::Decimal::from(4),
                "expected 10/3 decimal, got {n}"
            );
        }
        other => panic!("expected Number, got {other:?}"),
    }
}

#[test]
fn hourly_rate_times_date_range_yields_eur_total() {
    let code = r#"spec wage
uses lemma units
data money: quantity
  -> unit eur 1.00
data rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_hour eur/hour
data hourly_rate: 50 eur_per_hour
data period_start: 2026-01-01
data period_end: 2026-01-02
rule pay: (hourly_rate * (period_start...period_end as hours))"#;
    let value = eval_quantity_rule(code, "wage", "pay");
    let ValueKind::Quantity(amount, signature) = value.value else {
        panic!("expected quantity result, got {:?}", value.value);
    };
    assert_eq!(signature, vec![("eur".to_string(), 1)]);
    assert_eq!(
        lemma::ValueKind::Quantity(amount, signature.clone())
            .as_decimal_magnitude()
            .unwrap(),
        rust_decimal::Decimal::from(1200)
    );
}
