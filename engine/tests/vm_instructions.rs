//! Integration tests for the compiled-instruction VM (planning → execute_instructions).

use lemma::validate_instruction_jumps;
use lemma::{DateTimeValue, Engine, Instruction};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

const PRICING_SPEC: &str = r#"
spec pricing
data quantity: number
data price: 10
rule total: quantity * price
rule discount: 0
  unless quantity >= 10 then 5
  unless quantity >= 50 then 15
"#;

const CALC_SPEC: &str = r#"
spec calc

data money: quantity
  -> decimals 2
  -> unit eur 1

data hourly_rate: 85.00 eur
data hours_worked: 37.5
data is_rush: boolean
data is_super_rush: boolean

rule labor: hourly_rate * hours_worked
rule rush_surcharge: 0 eur
  unless is_rush then labor * 25%
  unless is_super_rush then labor * 50%
rule subtotal: labor + rush_surcharge
rule vat: subtotal * 21%
rule total: subtotal + vat
"#;

fn load_engine(code: &str, path: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(path))),
        )
        .expect("load spec");
    engine
}

fn assert_plan_jumps_strict(engine: &Engine, spec: &str) {
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, spec, Some(&now)).expect("plan");
    for rule in &plan.rules {
        validate_instruction_jumps(&rule.instructions.code);
    }
}

fn run_quantity(engine: &Engine, quantity: &str) -> lemma::Response {
    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("quantity".to_string(), quantity.to_string());
    engine
        .run(None, "pricing", Some(&now), data, false, None)
        .expect("run pricing")
}

fn run_calc(data: HashMap<String, String>) -> lemma::Response {
    let engine = load_engine(CALC_SPEC, "calc.lemma");
    let now = DateTimeValue::now();
    engine
        .run(None, "calc", Some(&now), data, false, None)
        .expect("run calc")
}

fn rule_number(response: &lemma::Response, rule: &str) -> Decimal {
    let result = response.get(rule).unwrap_or_else(|_| panic!("rule {rule}"));
    assert!(!result.vetoed, "rule {rule} vetoed");
    Decimal::from_str(result.number.as_ref().expect("number payload")).expect("decimal")
}

fn rule_display(response: &lemma::Response, rule: &str) -> String {
    response
        .get(rule)
        .unwrap_or_else(|_| panic!("rule {rule}"))
        .display
        .clone()
        .expect("display")
}

fn rule_quantity_display(response: &lemma::Response, rule: &str) -> String {
    response
        .get(rule)
        .unwrap_or_else(|_| panic!("rule {rule}"))
        .display
        .clone()
        .expect("display")
}

#[test]
fn pricing_plan_instructions_have_strict_jump_targets() {
    let engine = load_engine(PRICING_SPEC, "pricing.lemma");
    assert_plan_jumps_strict(&engine, "pricing");
    for rule in &engine
        .get_plan(None, "pricing", Some(&DateTimeValue::now()))
        .expect("plan")
        .rules
    {
        assert!(
            rule.instructions
                .code
                .iter()
                .any(|insn| matches!(insn, Instruction::Return { .. })),
            "rule '{}' must terminate with Return",
            rule.name
        );
    }
}

#[test]
fn calc_plan_instructions_have_strict_jump_targets() {
    let engine = load_engine(CALC_SPEC, "calc.lemma");
    assert_plan_jumps_strict(&engine, "calc");
}

#[test]
fn discount_unless_quantity_below_ten() {
    let engine = load_engine(PRICING_SPEC, "pricing.lemma");
    let response = run_quantity(&engine, "5");
    assert_eq!(rule_number(&response, "discount"), Decimal::ZERO);
    assert_eq!(rule_number(&response, "total"), Decimal::from(50));
}

#[test]
fn discount_unless_quantity_ten() {
    let engine = load_engine(PRICING_SPEC, "pricing.lemma");
    let response = run_quantity(&engine, "10");
    assert_eq!(rule_number(&response, "discount"), Decimal::from(5));
}

#[test]
fn discount_unless_quantity_fifty() {
    let engine = load_engine(PRICING_SPEC, "pricing.lemma");
    let response = run_quantity(&engine, "50");
    assert_eq!(rule_number(&response, "discount"), Decimal::from(15));
}

#[test]
fn discount_unless_quantity_between_tiers() {
    let engine = load_engine(PRICING_SPEC, "pricing.lemma");
    let response = run_quantity(&engine, "25");
    assert_eq!(rule_number(&response, "discount"), Decimal::from(5));
}

#[test]
fn nested_rule_reference_in_arithmetic() {
    let code = r#"
spec chain
data x: number
rule base: x * 2
rule offset: 1
  unless x >= 5 then 10
rule total: base + offset
"#;
    let engine = load_engine(code, "chain.lemma");
    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("x".to_string(), "3".to_string());
    let response = engine
        .run(None, "chain", Some(&now), data, false, None)
        .expect("run");
    assert_eq!(rule_number(&response, "total"), Decimal::from(7));
}

#[test]
fn inlined_piecewise_subexpression_does_not_return_early_from_parent_rule() {
    let mut data = HashMap::new();
    data.insert("is_rush".into(), "true".into());
    data.insert("is_super_rush".into(), "false".into());
    let response = run_calc(data);

    assert_eq!(rule_display(&response, "total"), "4821.09 eur");
    assert_eq!(
        rule_quantity_display(&response, "rush_surcharge"),
        "796.88 eur"
    );
    assert_ne!(
        rule_display(&response, "total"),
        rule_quantity_display(&response, "rush_surcharge"),
        "total must not equal inlined rush_surcharge alone"
    );
}

#[test]
fn is_veto_detects_transitive_veto() {
    let code = r#"
spec deep_veto
data input: number
rule step1: input
  unless input < 0 then veto "bad input"
rule step2: step1 * 2
rule step3: step2 + 1
rule check_step3: step3 is veto
"#;
    let engine = load_engine(code, "deep_veto.lemma");
    let now = DateTimeValue::now();
    let mut bad = HashMap::new();
    bad.insert("input".to_string(), "-1".to_string());
    let resp = engine
        .run(None, "deep_veto", Some(&now), bad, false, None)
        .expect("run");
    assert_eq!(rule_display(&resp, "check_step3"), "true");

    let mut good = HashMap::new();
    good.insert("input".to_string(), "5".to_string());
    let resp = engine
        .run(None, "deep_veto", Some(&now), good, false, None)
        .expect("run");
    assert_eq!(rule_display(&resp, "check_step3"), "false");
}

#[test]
fn unless_skips_vetoed_condition_and_matches_is_veto_arm() {
    let code = r#"
spec unless_veto_cond
data input: number
rule validated: input
  unless input > 1000 then veto "too large"
rule result: 0
  unless validated > 50 then 100
  unless validated is veto then 0
"#;
    let engine = load_engine(code, "unless_veto.lemma");
    let now = DateTimeValue::now();
    let mut large = HashMap::new();
    large.insert("input".to_string(), "2000".to_string());
    let resp = engine
        .run(None, "unless_veto_cond", Some(&now), large, false, None)
        .expect("run");
    assert_eq!(rule_display(&resp, "result"), "0");

    let mut ok = HashMap::new();
    ok.insert("input".to_string(), "100".to_string());
    let resp = engine
        .run(None, "unless_veto_cond", Some(&now), ok, false, None)
        .expect("run");
    assert_eq!(rule_display(&resp, "result"), "100");
}

#[test]
fn calc_total_without_rush_is_not_zero_surcharge_path() {
    let mut data = HashMap::new();
    data.insert("is_rush".into(), "false".into());
    data.insert("is_super_rush".into(), "false".into());
    let response = run_calc(data);
    assert_eq!(rule_display(&response, "total"), "3856.88 eur");
    assert_eq!(
        rule_quantity_display(&response, "rush_surcharge"),
        "0.00 eur"
    );
}

/// Serialize the pricing plan, tamper with one rule's instructions through the
/// serialized JSON, and reconstruct. The trust boundary must reject the plan
/// with an error — never hang or crash the virtual machine later.
fn tampered_plan_error(tamper: impl FnOnce(&mut lemma::Instructions)) -> String {
    let engine = load_engine(PRICING_SPEC, "pricing.lemma");
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "pricing", Some(&now)).expect("plan");
    let mut serialized = lemma::ExecutionPlanSerialized::from(plan);
    tamper(
        &mut serialized
            .rules
            .first_mut()
            .expect("pricing plan has rules")
            .instructions,
    );
    let error = lemma::ExecutionPlan::try_from(serialized)
        .expect_err("tampered serialized plan must be rejected at load time");
    error.to_string()
}

#[test]
fn deserialization_rejects_cyclic_jump() {
    let message = tampered_plan_error(|instructions| {
        instructions.code.insert(
            0,
            Instruction::Jump {
                target_instruction: 0,
            },
        );
    });
    assert!(
        message.contains("unpatched Jump"),
        "expected jump validation error, got: {message}"
    );
}

#[test]
fn deserialization_rejects_out_of_bounds_constant_index() {
    let message = tampered_plan_error(|instructions| {
        for instruction in &mut instructions.code {
            if let Instruction::LoadConstant { constant_index, .. } = instruction {
                *constant_index = u16::MAX;
            }
        }
    });
    assert!(
        message.contains("constant index"),
        "expected constant index validation error, got: {message}"
    );
}

#[test]
fn deserialization_rejects_missing_trailing_return() {
    let message = tampered_plan_error(|instructions| {
        while matches!(instructions.code.last(), Some(Instruction::Return { .. })) {
            instructions.code.pop();
        }
    });
    assert!(
        message.contains("must end with Return") || message.contains("past the last instruction"),
        "expected trailing-return validation error, got: {message}"
    );
}
