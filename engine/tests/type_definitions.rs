use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, TypeSpecification};
use std::collections::HashMap;

#[test]
fn test_type_system_with_imports_and_extensions() {
    let mut engine = Engine::new();

    let age_spec = r#"
spec age
data age: number
  -> minimum 0
  -> maximum 150
"#;

    let test_types_spec = r#"
spec test_types

data age from age

data adult_age: age
  -> minimum 21

data twenties: adult_age -> maximum 30

rule total: age + adult_age + twenties
"#;

    engine
        .load(age_spec, lemma::SourceType::Labeled("age.lemma"))
        .unwrap();
    engine
        .load(
            test_types_spec,
            lemma::SourceType::Labeled("test_types.lemma"),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let mut data = HashMap::new();
    data.insert("age".to_string(), "25".to_string());
    data.insert("adult_age".to_string(), "30".to_string());
    data.insert("twenties".to_string(), "25".to_string());

    let response = engine
        .run("test_types", Some(&now), data, false)
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

    engine
        .load(
            r#"
        spec pricing
        data money: scale
          -> default 4 eur
          -> unit eur 1
          -> unit usd 1.19
        data price: money
        rule doubled: price * 2
    "#,
            lemma::SourceType::Labeled("pricing.lemma"),
        )
        .expect("default before unit should be valid");
    let now = DateTimeValue::now();

    let plan = engine.get_plan("pricing", Some(&now)).unwrap();
    let schema = plan.schema();
    let entry = schema.data.get("price").expect("price data in schema");
    assert!(
        entry.lemma_type.is_scale(),
        "price must be scale money type"
    );
    assert_eq!(entry.lemma_type.name(), "money");
    match &entry.lemma_type.specifications {
        TypeSpecification::Scale { units, .. } => {
            let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            assert!(names.contains(&"eur") && names.contains(&"usd"));
        }
        other => panic!("expected Scale, got {:?}", other),
    }
    assert!(
        entry.default.is_some(),
        "typedef default 4 eur must be promoted into price binding"
    );
}

/// Verify that `-> default` after `-> unit` (the original order) still works.
#[test]
fn test_scale_type_default_after_unit_declarations() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec pricing
        data money: scale
          -> unit eur 1
          -> unit usd 1.19
          -> default 4 eur
        data price: money
        rule doubled: price * 2
    "#,
            lemma::SourceType::Labeled("pricing.lemma"),
        )
        .expect("default after unit should be valid");
    let now = DateTimeValue::now();

    let plan = engine.get_plan("pricing", Some(&now)).unwrap();
    let schema = plan.schema();
    let entry = schema.data.get("price").expect("price data in schema");
    assert!(
        entry.lemma_type.is_scale(),
        "price must be scale money type"
    );
    assert_eq!(entry.lemma_type.name(), "money");
    match &entry.lemma_type.specifications {
        TypeSpecification::Scale { units, .. } => {
            let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            assert!(names.contains(&"eur") && names.contains(&"usd"));
        }
        other => panic!("expected Scale, got {:?}", other),
    }
    assert!(
        entry.default.is_some(),
        "typedef default 4 eur must be promoted into price binding"
    );
}

#[test]
fn test_schema_returns_data_in_definition_order() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec ordering
        data zebra: number
        data alpha: number
        data middle: number
        rule total: zebra + alpha + middle
    "#,
            lemma::SourceType::Labeled("ordering.lemma"),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("ordering", Some(&now)).unwrap();
    let schema = plan.schema();
    let data_names: Vec<&String> = schema.data.keys().collect();
    assert_eq!(
        data_names,
        vec!["zebra", "alpha", "middle"],
        "Data should be in definition order, not alphabetical"
    );
}

#[test]
fn test_schema_for_rules_returns_data_in_definition_order() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec ordering
        data zebra: number
        data alpha: number
        data middle: number
        rule total: zebra + alpha + middle
    "#,
            lemma::SourceType::Labeled("ordering.lemma"),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("ordering", Some(&now)).unwrap();
    let schema = plan.schema_for_rules(&["total".to_string()]).unwrap();
    let data_names: Vec<&String> = schema.data.keys().collect();
    assert_eq!(
        data_names,
        vec!["zebra", "alpha", "middle"],
        "schema_for_rules should also preserve definition order"
    );
}

#[test]
fn test_schema_default_valued_data_are_values() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec defaults
        data quantity: number -> default 10
        data name: text
        data price: 99
        rule total: quantity * price
        rule label: name
    "#,
            lemma::SourceType::Labeled("defaults.lemma"),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("defaults", Some(&now)).unwrap();
    let schema = plan.schema();

    let quantity = schema.data.get("quantity").expect("quantity should exist");
    assert!(
        quantity.default.is_some(),
        "Type default promotes to value in execution plan"
    );

    let name = schema.data.get("name").expect("name should exist");
    assert!(
        name.default.is_none(),
        "Type-only data without default has no value"
    );

    let price = schema.data.get("price").expect("price should exist");
    assert!(price.default.is_some(), "Explicit literal is a value");
}

#[test]
fn test_schema_scale_default_is_value() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec salary
        data money: scale
          -> unit eur 1
          -> unit usd 1.19
          -> default 3000 eur
        data salary: money
        rule doubled: salary * 2
    "#,
            lemma::SourceType::Labeled("salary.lemma"),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("salary", Some(&now)).unwrap();
    let schema = plan.schema();

    let salary = schema.data.get("salary").expect("salary should exist");
    assert!(
        salary.default.is_some(),
        "Scale type default promotes to value in execution plan"
    );
}

/// Default declared on an inner typedef must propagate through all extending
/// types and land on the data binding's default, without the intermediate
/// types redeclaring it.
#[test]
fn test_typedef_default_inherits_through_extension_chain() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
            spec chain
            data money: scale
              -> unit eur 1
              -> default 4 eur
            data price: money
            data final_price: price
            rule doubled: final_price * 2
            "#,
            lemma::SourceType::Labeled("chain.lemma"),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let schema = engine.get_plan("chain", Some(&now)).unwrap().schema();
    let final_price = schema
        .data
        .get("final_price")
        .expect("final_price should exist");
    assert!(
        final_price.default.is_some(),
        "typedef default declared on ancestor type must inherit down to leaf binding"
    );
}
