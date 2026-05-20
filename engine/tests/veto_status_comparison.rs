//! `is veto` / `ResultIsVeto` — boolean coercion from operand `OperationResult`.

use lemma::evaluation::operations::ComputationKind;
use lemma::explanation::ExplanationNode;
use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, OperationResult, VetoType};
use std::collections::HashMap;

#[test]
fn unless_validated_price_is_veto_then_fallback_value() {
    let code = r#"
spec pricing
data price: -5
data quantity: 2
rule validated_price: price
    unless price < 0 then veto "Price cannot be negative"
rule total: validated_price * quantity
    unless validated_price is veto then 0
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule");

    assert_eq!(
        total.result,
        OperationResult::Value(Box::new(lemma::LiteralValue::number_from_decimal(
            rust_decimal::Decimal::ZERO
        )))
    );
}

#[test]
fn unless_re_veto_uses_outer_message_only() {
    let code = r#"
spec pricing
data price: -5
rule validated_price: price
    unless price < 0 then veto "Price cannot be negative"
rule total: validated_price
    unless validated_price is veto then veto "Upstream failed"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule");

    assert_eq!(
        total.result,
        OperationResult::Veto(VetoType::UserDefined {
            message: Some("Upstream failed".to_string())
        })
    );
}

#[test]
fn rule_reference_is_veto_when_rule_vetoed() {
    let code = r#"
spec pricing
data quantity: -1
rule validated_quantity: quantity
    unless quantity < 0 then veto "Quantity cannot be negative"
rule flag: validated_quantity is veto
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let flag = response
        .results
        .values()
        .find(|r| r.rule.name == "flag")
        .expect("flag");

    assert_eq!(
        flag.result,
        OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );
}

#[test]
fn unless_on_rule_reference_is_veto_uses_rule_result_not_inlined_body() {
    let code = r#"
spec pricing
data price: 10
data quantity: -1
rule validated_quantity: quantity
    unless quantity < 0 then veto "Quantity cannot be negative"
rule total: price * validated_quantity
    unless validated_quantity is veto then 0
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total");

    assert_eq!(
        total.result,
        OperationResult::Value(Box::new(lemma::LiteralValue::number_from_decimal(
            rust_decimal::Decimal::ZERO
        )))
    );
}

#[test]
fn operand_is_veto_false_when_operand_has_value() {
    let code = r#"
spec pricing
data price: 10
rule validated_price: price
    unless price < 0 then veto "Price cannot be negative"
rule flag: validated_price is veto
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let flag = response
        .results
        .values()
        .find(|r| r.rule.name == "flag")
        .expect("flag");

    assert_eq!(
        flag.result,
        OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(false)))
    );
}

#[test]
fn commutative_veto_is_validated_price_matches_is_veto() {
    let code = r#"
spec pricing
data price: -5
rule validated_price: price
    unless price < 0 then veto "negative"
rule left_to_right: validated_price is veto
rule right_to_left: veto is validated_price
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let ltr = response
        .results
        .values()
        .find(|r| r.rule.name == "left_to_right")
        .expect("left_to_right");
    let rtl = response
        .results
        .values()
        .find(|r| r.rule.name == "right_to_left")
        .expect("right_to_left");

    assert_eq!(ltr.result, rtl.result);
    assert_eq!(
        ltr.result,
        OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );
}

#[test]
fn not_veto_is_x_matches_is_not_veto() {
    let code = r#"
spec pricing
data price: 10
rule validated_price: price
rule a: validated_price is not veto
rule b: not veto is validated_price
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let a = response
        .results
        .values()
        .find(|r| r.rule.name == "a")
        .expect("a");
    let b = response
        .results
        .values()
        .find(|r| r.rule.name == "b")
        .expect("b");
    assert_eq!(a.result, b.result);
    assert_eq!(
        a.result,
        OperationResult::Value(Box::new(lemma::LiteralValue::from_bool(true)))
    );
}

#[test]
fn explanation_records_result_is_veto_on_unless_fallback() {
    let code = r#"
spec pricing
data price: -5
data quantity: 1
rule validated_price: price
    unless price < 0 then veto "negative"
rule total: validated_price * quantity
    unless validated_price is veto then 0
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total");
    let explanation = total.explanation.as_ref().expect("explanation");

    let ExplanationNode::Branches { matched, .. } = explanation.tree.as_ref() else {
        panic!("expected Branches explanation, got {:?}", explanation.tree);
    };
    let condition = matched
        .condition
        .as_ref()
        .expect("matched unless should have condition");
    let ExplanationNode::Computation { kind, .. } = condition.as_ref() else {
        panic!("expected Computation on unless condition, got {condition:?}");
    };
    assert_eq!(*kind, ComputationKind::ResultIsVeto);
}

#[test]
fn parse_rejects_veto_with_message_in_is_veto_comparison() {
    let code = r#"
spec bad
data x: 1
rule r: x is veto "no"
"#;

    let mut engine = Engine::new();
    let err = engine
        .load(code, lemma::SourceType::Volatile)
        .expect_err("veto message in is veto position must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("veto with a message") || msg.contains("is veto"),
        "unexpected error: {msg}"
    );
}
