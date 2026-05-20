//! API rule-result quantity and ratio unit conversion.

use lemma::evaluation::explanation::ExplanationNode;
use lemma::evaluation::{ComputationKind, OperationKind, OperationRecord};
use lemma::{EvaluationRequest, OperationResult, RationalInteger, SourceType, ValueKind};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;

fn tree_contains_arithmetic(node: &ExplanationNode) -> bool {
    match node {
        ExplanationNode::Computation {
            kind: ComputationKind::Arithmetic(_),
            ..
        } => true,
        ExplanationNode::Computation { operands, .. } => {
            operands.iter().any(tree_contains_arithmetic)
        }
        ExplanationNode::Branches { matched, .. } => {
            tree_contains_arithmetic(matched.result.as_ref())
        }
        ExplanationNode::RuleReference { expansion, .. } => {
            tree_contains_arithmetic(expansion.as_ref())
        }
        _ => false,
    }
}

fn unit_conversion_operation_count(operations: &[OperationRecord]) -> usize {
    operations
        .iter()
        .filter(|op| {
            matches!(
                op.kind,
                OperationKind::Computation {
                    kind: ComputationKind::UnitConversion { .. },
                    ..
                }
            )
        })
        .count()
}

const SPEC: &str = r#"
spec money
data price: quantity
  -> unit eur 1
  -> unit usd 0.91
  -> default 100 eur
rule total: price
rule tax: total * 10%
rule ok_flag: total > 50 eur
"#;

fn load_engine() -> lemma::Engine {
    let mut engine = lemma::Engine::new();
    engine
        .load(
            SPEC,
            SourceType::Path(Arc::new(std::path::PathBuf::from("money.lemma"))),
        )
        .expect("spec loads");
    engine
}

fn run_with_request(engine: &lemma::Engine, request: EvaluationRequest) -> lemma::Response {
    run_spec_with_request(engine, "money", request)
}

fn run_spec_with_request(
    engine: &lemma::Engine,
    spec: &str,
    request: EvaluationRequest,
) -> lemma::Response {
    let now = lemma::DateTimeValue::now();
    engine
        .run(None, spec, Some(&now), HashMap::new(), false, request)
        .expect("run")
}

#[test]
fn converts_quantity_rule_result_to_declared_unit() {
    let engine = load_engine();
    let request = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "usd".to_string())]),
        engine
            .get_plan(None, "money", Some(&lemma::DateTimeValue::now()))
            .expect("plan"),
    )
    .expect("valid conversion");
    let response = run_with_request(&engine, request);
    let total = response.results.get("total").expect("total");
    match &total.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Quantity(_, unit, _) => assert_eq!(unit, "usd"),
            other => panic!("expected quantity, got {other:?}"),
        },
        OperationResult::Veto(_) => panic!("total should not veto"),
    }
}

#[test]
fn rejects_unit_not_in_type_valid_units_list() {
    let engine = load_engine();
    let plan = engine
        .get_plan(None, "money", Some(&lemma::DateTimeValue::now()))
        .expect("plan");
    let err = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "gbp".to_string())]),
        plan,
    )
    .expect_err("gbp not on Money");
    let message = err.to_string();
    assert!(message.contains("Valid units: eur, usd"), "got: {message}");
}

#[test]
fn rejects_reserved_number_target() {
    let engine = load_engine();
    let plan = engine
        .get_plan(None, "money", Some(&lemma::DateTimeValue::now()))
        .expect("plan");
    let err = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "number".to_string())]),
        plan,
    )
    .expect_err("number reserved");
    assert!(err
        .to_string()
        .contains("quantity or ratio unit names only"));
}

#[test]
fn rejects_non_unit_bearing_rule_conversion() {
    let engine = load_engine();
    let plan = engine
        .get_plan(None, "money", Some(&lemma::DateTimeValue::now()))
        .expect("plan");
    let err = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("ok_flag".to_string(), "usd".to_string())]),
        plan,
    )
    .expect_err("boolean rule");
    assert!(err.to_string().contains("quantity or ratio result type"));
}

const RATE_SPEC: &str = r#"
spec rates
data rate: ratio
  -> unit basis_points 10000
  -> unit percent 100
  -> default 500 basis_points
rule rate_out: rate
"#;

fn load_rate_engine() -> lemma::Engine {
    let mut engine = lemma::Engine::new();
    engine
        .load(
            RATE_SPEC,
            SourceType::Path(Arc::new(std::path::PathBuf::from("rates.lemma"))),
        )
        .expect("rates spec loads");
    engine
}

#[test]
fn converts_ratio_rule_result_unit_label() {
    let engine = load_rate_engine();
    let plan = engine
        .get_plan(None, "rates", Some(&lemma::DateTimeValue::now()))
        .expect("plan");
    let request = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("rate_out".to_string(), "percent".to_string())]),
        plan,
    )
    .expect("percent valid for rate");
    let response = run_spec_with_request(&engine, "rates", request);
    let rate_out = response.results.get("rate_out").expect("rate_out");
    match &rate_out.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Ratio(r, unit) => {
                assert_eq!(unit.as_deref(), Some("percent"));
                assert_eq!(*r, RationalInteger::new(1, 20));
            }
            other => panic!("expected ratio, got {other:?}"),
        },
        OperationResult::Veto(_) => panic!("rate_out should not veto"),
    }
}

#[test]
fn rejects_quantity_unit_on_ratio_rule() {
    let engine = load_rate_engine();
    let plan = engine
        .get_plan(None, "rates", Some(&lemma::DateTimeValue::now()))
        .expect("plan");
    let err = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("rate_out".to_string(), "eur".to_string())]),
        plan,
    )
    .expect_err("eur not on rate");
    let message = err.to_string();
    assert!(
        message.contains("Valid units:") || message.contains("Unknown unit"),
        "got: {message}"
    );
}

#[test]
fn rejects_ratio_unit_on_quantity_rule() {
    let engine = load_engine();
    let plan = engine
        .get_plan(None, "money", Some(&lemma::DateTimeValue::now()))
        .expect("plan");
    let err = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "percent".to_string())]),
        plan,
    )
    .expect_err("percent not on money");
    let message = err.to_string();
    assert!(
        message.contains("Valid units:") || message.contains("does not belong to a quantity type"),
        "got: {message}"
    );
}

const PRICING_SPEC: &str = r#"
spec pricing
data money: quantity -> unit eur 1 -> unit usd 0.91
data mass: quantity -> unit kilogram 1
data amount: mass
data cost_per_unit: quantity -> unit eur_per_kilo eur/kilogram
rule total: (amount * cost_per_unit) as eur
"#;

const PRICING_SI_CURRENCY_SPEC: &str = r#"
spec pricing

uses lemma si

data currency: quantity
  -> unit eur 1
  -> unit usd 0.84
  -> decimals 2

data amount: si.mass
data cost_per_unit: quantity
  -> unit eur_per_kilo eur/kilogram
  -> default 1.50 eur_per_kilo
  -> minimum 1.20 eur_per_kilo
  -> maximum 2.00 eur_per_kilo
  -> decimals 2

rule total: (cost_per_unit * amount) as usd
"#;

fn load_pricing_engine() -> lemma::Engine {
    let mut engine = lemma::Engine::new();
    engine
        .load(
            PRICING_SPEC,
            SourceType::Path(Arc::new(std::path::PathBuf::from("pricing.lemma"))),
        )
        .expect("pricing spec loads");
    engine
}

fn load_pricing_si_currency_engine() -> lemma::Engine {
    let mut engine = lemma::Engine::new();
    engine
        .load(
            PRICING_SI_CURRENCY_SPEC,
            SourceType::Path(Arc::new(std::path::PathBuf::from("pricing_si.lemma"))),
        )
        .expect("pricing si currency spec loads");
    engine
}

#[test]
fn pricing_rule_result_unit_conversion_eur_to_usd() {
    let engine = load_pricing_engine();
    let now = lemma::DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("amount".to_string(), "12 kilogram".to_string());
    data.insert("cost_per_unit".to_string(), "1.50 eur_per_kilo".to_string());
    let request = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "usd".to_string())]),
        engine.get_plan(None, "pricing", Some(&now)).expect("plan"),
    )
    .expect("usd valid for money rule");
    let response = engine
        .run(None, "pricing", Some(&now), data, false, request)
        .expect("run must not panic");
    let total = response.results.get("total").expect("total");
    match &total.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Quantity(n, unit, _) => {
                assert_eq!(unit, "usd");
                let expected = Decimal::new(1800, 2)
                    .checked_div(Decimal::new(91, 2))
                    .expect("18 eur as usd");
                assert_eq!(
                    lemma::commit_rational_to_decimal(n).unwrap(),
                    expected,
                    "12 kg * 1.50 eur/kg = 18 eur -> usd"
                );
            }
            other => panic!("expected quantity, got {other:?}"),
        },
        OperationResult::Veto(_) => panic!("total should not veto"),
    }
}

#[test]
fn pricing_si_currency_rule_as_usd_api_display_eur() {
    let engine = load_pricing_si_currency_engine();
    let now = lemma::DateTimeValue::now();
    let data = HashMap::from([("amount".to_string(), "12 kilogram".to_string())]);
    let plan = engine.get_plan(None, "pricing", Some(&now)).expect("plan");
    let request = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "eur".to_string())]),
        plan,
    )
    .expect("eur valid on currency rule type");
    let response = engine
        .run(None, "pricing", Some(&now), data, false, request)
        .expect("run must not panic");
    let total = response.results.get("total").expect("total");
    match &total.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Quantity(n, unit, _) => {
                assert_eq!(unit, "eur", "API display conversion target");
                // 12 kg * 1.50 eur/kg = 18 eur; rule evaluates as usd, request shows eur
                assert_eq!(
                    lemma::commit_rational_to_decimal(n).unwrap(),
                    Decimal::new(1800, 2),
                    "total in eur"
                );
            }
            other => panic!("expected quantity, got {other:?}"),
        },
        OperationResult::Veto(_) => panic!("total should not veto: {total:?}"),
    }
}

#[test]
fn pricing_rejects_incompatible_rule_result_unit_at_request() {
    let engine = load_pricing_engine();
    let plan = engine
        .get_plan(None, "pricing", Some(&lemma::DateTimeValue::now()))
        .expect("plan");
    let err = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "kilogram".to_string())]),
        plan,
    )
    .expect_err("kilogram not a money unit");
    let message = err.to_string();
    assert!(
        message.contains("Valid units: eur") || message.contains("Cannot convert"),
        "got: {message}"
    );
}

#[test]
fn rule_result_units_preserves_evaluation_explanation_tree() {
    let engine = load_pricing_engine();
    let now = lemma::DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("amount".to_string(), "12 kilogram".to_string());
    data.insert("cost_per_unit".to_string(), "1.50 eur_per_kilo".to_string());
    let request = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "usd".to_string())]),
        engine.get_plan(None, "pricing", Some(&now)).expect("plan"),
    )
    .expect("usd valid");
    let response = engine
        .run(None, "pricing", Some(&now), data, false, request)
        .expect("run");
    let total = response.results.get("total").expect("total");
    match &total.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Quantity(_, unit, _) => assert_eq!(unit, "usd"),
            other => panic!("expected quantity, got {other:?}"),
        },
        OperationResult::Veto(_) => panic!("total should not veto"),
    }
    let explanation = total.explanation.as_ref().expect("explanation");
    match explanation.result {
        OperationResult::Value(ref v) => match &v.value {
            ValueKind::Quantity(_, unit, _) => assert_eq!(unit, "usd"),
            other => panic!("explanation.result unit, got {other:?}"),
        },
        OperationResult::Veto(_) => panic!("explanation.result should not veto"),
    }
    let tree = explanation.tree.as_ref();
    if let ExplanationNode::Computation {
        expression,
        original_expression,
        ..
    } = tree
    {
        assert_ne!(
            expression, "total as usd",
            "API display must not replace audit tree with synthetic label"
        );
        assert_ne!(original_expression, "total as usd");
    }
    assert!(
        tree_contains_arithmetic(tree),
        "audit tree must still contain multiply from (amount * cost_per_unit)"
    );
}

#[test]
fn rule_result_units_does_not_record_display_conversion_operation() {
    let engine = load_pricing_engine();
    let now = lemma::DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("amount".to_string(), "12 kilogram".to_string());
    data.insert("cost_per_unit".to_string(), "1.50 eur_per_kilo".to_string());
    let plan = engine.get_plan(None, "pricing", Some(&now)).expect("plan");
    let baseline = engine
        .run(
            None,
            "pricing",
            Some(&now),
            data.clone(),
            true,
            EvaluationRequest::default(),
        )
        .expect("run");
    let request = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "usd".to_string())]),
        plan,
    )
    .expect("usd valid");
    let with_display = engine
        .run(None, "pricing", Some(&now), data, true, request)
        .expect("run");
    let baseline_ops = unit_conversion_operation_count(
        baseline
            .results
            .get("total")
            .expect("total")
            .operations
            .as_slice(),
    );
    let display_ops = unit_conversion_operation_count(
        with_display
            .results
            .get("total")
            .expect("total")
            .operations
            .as_slice(),
    );
    assert_eq!(
        baseline_ops, display_ops,
        "API display conversion must not add UnitConversion operations"
    );
}

#[test]
fn dependency_sees_unconverted_rule_result() {
    let engine = load_engine();
    let request = EvaluationRequest::from_rule_conversion_strings(
        HashMap::from([("total".to_string(), "usd".to_string())]),
        engine
            .get_plan(None, "money", Some(&lemma::DateTimeValue::now()))
            .expect("plan"),
    )
    .expect("valid");
    let response = run_with_request(&engine, request);
    let tax = response.results.get("tax").expect("tax");
    match &tax.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Quantity(n, unit, _) => {
                assert_eq!(unit, "eur", "tax uses unconverted total (100 eur)");
                assert_eq!(
                    lemma::commit_rational_to_decimal(n).unwrap(),
                    Decimal::new(1000, 2),
                    "10% of 100 eur"
                );
            }
            other => panic!("expected quantity, got {other:?}"),
        },
        OperationResult::Veto(_) => panic!("tax should not veto"),
    }
    let total = response.results.get("total").expect("total");
    match &total.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Quantity(n, unit, _) => {
                assert_eq!(unit, "usd");
                let expected = Decimal::new(10000, 2)
                    .checked_div(Decimal::new(91, 2))
                    .expect("100 eur as usd with factor 0.91 (1 usd = 0.91 eur)");
                assert_eq!(lemma::commit_rational_to_decimal(n).unwrap(), expected);
            }
            other => panic!("expected usd quantity, got {other:?}"),
        },
        OperationResult::Veto(_) => panic!("total should not veto"),
    }
}
