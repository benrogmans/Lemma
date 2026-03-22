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
        .run("test_types", Some(&now), facts, false)
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

    let plan = engine.get_plan("pricing", Some(&now)).unwrap();
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

    let plan = engine.get_plan("pricing", Some(&now)).unwrap();
    let schema = plan.schema();
    assert!(
        schema.facts.contains_key("price"),
        "price fact should exist"
    );
}

#[test]
fn test_schema_returns_facts_in_definition_order() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec ordering
        fact zebra: [number]
        fact alpha: [number]
        fact middle: [number]
        rule total: zebra + alpha + middle
    "#,
        "ordering.lemma",
    )
    .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("ordering", Some(&now)).unwrap();
    let schema = plan.schema();
    let fact_names: Vec<&String> = schema.facts.keys().collect();
    assert_eq!(
        fact_names,
        vec!["zebra", "alpha", "middle"],
        "Facts should be in definition order, not alphabetical"
    );
}

#[test]
fn test_schema_for_rules_returns_facts_in_definition_order() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec ordering
        fact zebra: [number]
        fact alpha: [number]
        fact middle: [number]
        rule total: zebra + alpha + middle
    "#,
        "ordering.lemma",
    )
    .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("ordering", Some(&now)).unwrap();
    let schema = plan.schema_for_rules(&["total".to_string()]).unwrap();
    let fact_names: Vec<&String> = schema.facts.keys().collect();
    assert_eq!(
        fact_names,
        vec!["zebra", "alpha", "middle"],
        "schema_for_rules should also preserve definition order"
    );
}

#[test]
fn test_schema_reports_none_for_default_valued_facts() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec defaults
        fact quantity: [number -> default 10]
        fact name: [text]
        fact price: 99
        rule total: quantity * price
    "#,
        "defaults.lemma",
    )
    .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("defaults", Some(&now)).unwrap();
    let schema = plan.schema();

    let (_, quantity_val) = schema.facts.get("quantity").expect("quantity should exist");
    assert!(
        quantity_val.is_none(),
        "Default-valued fact 'quantity' should have None in schema (needs user input)"
    );

    let (_, name_val) = schema.facts.get("name").expect("name should exist");
    assert!(
        name_val.is_none(),
        "Type-only fact 'name' should have None in schema"
    );

    let (_, price_val) = schema.facts.get("price").expect("price should exist");
    assert!(
        price_val.is_some(),
        "Explicit-valued fact 'price' should have Some in schema (skip in interactive)"
    );
}

#[test]
fn test_schema_scale_default_reports_none() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec salary
        type money: scale
          -> unit eur 1
          -> unit usd 1.19
          -> default 3000 eur
        fact salary: [money]
        rule doubled: salary * 2
    "#,
        "salary.lemma",
    )
    .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("salary", Some(&now)).unwrap();
    let schema = plan.schema();

    let (_, salary_val) = schema.facts.get("salary").expect("salary should exist");
    assert!(
        salary_val.is_none(),
        "Scale fact with type default should have None in schema"
    );
}
