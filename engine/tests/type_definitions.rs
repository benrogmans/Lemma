use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

#[test]
fn test_type_system_with_imports_and_extensions() {
    let mut engine = Engine::new();

    let age_spec = r#"
spec age
type age: number
  -> minimum 0
  -> maximum 150
"#;

    let test_types_spec = r#"
spec test_types

type age from age

type adult_age: age
  -> minimum 21

fact age: [age]
fact adult_age: [adult_age]
fact twenties: [adult_age -> maximum 30]

rule total: age + adult_age + twenties
"#;

    add_lemma_code_blocking(&mut engine, age_spec, "age.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, test_types_spec, "test_types.lemma").unwrap();
    let now = DateTimeValue::now();

    let mut facts = HashMap::new();
    facts.insert("age".to_string(), "25".to_string());
    facts.insert("adult_age".to_string(), "30".to_string());
    facts.insert("twenties".to_string(), "25".to_string());

    let response = engine
        .evaluate("test_types", None, &now, vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.spec_name, "test_types");

    let total_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule not found");

    // 25 + 30 + 25 = 80
    assert_eq!(total_rule.result.value().unwrap().to_string(), "80");
}

/// Regression test: scale type with `-> default` before `-> unit` must work.
/// Previously, constraints were applied in declaration order, so `default`
/// would fail to find the unit because it hadn't been registered yet.
#[test]
fn test_scale_type_default_before_unit_declarations() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec pricing
        type money: scale
          -> default 4 eur
          -> unit eur 1
          -> unit usd 1.19
        fact price: [money]
        rule doubled: price * 2
    "#,
        "pricing.lemma",
    )
    .expect("default before unit should be valid");
    let now = DateTimeValue::now();

    let plan = engine.get_execution_plan("pricing", None, &now).unwrap();
    let schema = plan.schema();
    assert!(
        schema.facts.contains_key("price"),
        "price fact should exist"
    );
}

/// Verify that `-> default` after `-> unit` (the original order) still works.
#[test]
fn test_scale_type_default_after_unit_declarations() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec pricing
        type money: scale
          -> unit eur 1
          -> unit usd 1.19
          -> default 4 eur
        fact price: [money]
        rule doubled: price * 2
    "#,
        "pricing.lemma",
    )
    .expect("default after unit should be valid");
    let now = DateTimeValue::now();

    let plan = engine.get_execution_plan("pricing", None, &now).unwrap();
    let schema = plan.schema();
    assert!(
        schema.facts.contains_key("price"),
        "price fact should exist"
    );
}
