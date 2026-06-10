use lemma::DateTimeValue;
use lemma::{Engine, TypeSpecification};
use std::collections::HashMap;

#[test]
fn test_missing_uses_for_data_reference_fails() {
    let mut engine = Engine::new();

    let money_spec = r#"
spec money
data salary: number
"#;

    let test_spec = r#"
spec test
with salary: money.salary
rule total: salary
"#;

    engine
        .load(
            money_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("money.lemma"))),
        )
        .unwrap();
    let err = engine
        .load(test_spec, lemma::SourceType::Volatile)
        .unwrap_err();

    let err_msg = err.errors[0].to_string();
    assert!(
        err_msg.contains("imported spec") || err_msg.contains("alias.field"),
        "local with without import path must be rejected at parse: {}",
        err_msg
    );
}

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

uses age_spec: age
data age: age_spec.age

data adult_age: age
  -> minimum 21

data twenties: adult_age -> maximum 30

rule total: age + adult_age + twenties
"#;

    engine
        .load(
            age_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("age.lemma"))),
        )
        .unwrap();
    engine
        .load(
            test_types_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "test_types.lemma",
            ))),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let mut data = HashMap::new();
    data.insert("age".to_string(), "25".to_string());
    data.insert("adult_age".to_string(), "30".to_string());
    data.insert("twenties".to_string(), "25".to_string());
    let response = engine
        .run(None, "test_types", Some(&now), data, false, None)
        .expect("Evaluation failed");

    assert_eq!(response.spec_name, "test_types");

    let total_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule not found");

    // 25 + 30 + 25 = 80
    assert_eq!(total_rule.display.clone().expect("display"), "80");
}

#[test]
fn test_import_literal_number_via_from_dependency_spec() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec constants
data pi: 3.14

spec finance
uses constants
data pi: constants.pi
rule x: pi
"#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "workspace.lemma",
            ))),
        )
        .expect("loading specs with literal + from-import must succeed");

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "finance", Some(&now), HashMap::new(), false, None)
        .expect("run finance");

    let rule_x = response.results.get("x").expect("rule x");
    assert_eq!(rule_x.display.clone().expect("display"), "3.14");
}

/// Regression test: quantity type with `-> default` before `-> unit` must work.
/// Previously, constraints were applied in declaration order, so `default`
/// would fail to find the unit because it hadn't been registered yet.
#[test]
fn test_quantity_type_default_before_unit_declarations() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec pricing
        data money: quantity
          -> default 4 eur
          -> unit eur 1
          -> unit usd 0.84
        data price: money
        rule doubled: price * 2
    "#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "pricing.lemma",
            ))),
        )
        .expect("default before unit should be valid");
    let now = DateTimeValue::now();

    let plan = engine.get_plan(None, "pricing", Some(&now)).unwrap();
    let schema = plan.schema(&lemma::DataOverlay::default());
    let entry = schema.data.get("price").expect("price data in schema");
    assert!(
        entry.lemma_type.is_quantity(),
        "price must be quantity money type"
    );
    assert_eq!(entry.lemma_type.extends.parent_name(), Some("money"));
    match &entry.lemma_type.specifications {
        TypeSpecification::Quantity { units, .. } => {
            let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            assert!(names.contains(&"eur") && names.contains(&"usd"));
        }
        other => panic!("expected Quantity, got {:?}", other),
    }
    assert!(
        entry.default.is_some() && entry.bound_value.is_none(),
        "typedef money default must surface on price as schema suggestion"
    );
}

/// Verify that `-> default` after `-> unit` (the original order) still works.
#[test]
fn test_quantity_type_default_after_unit_declarations() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec pricing
        data money: quantity
          -> unit eur 1
          -> unit usd 0.84
          -> default 4 eur
        rule doubled: money * 2
    "#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "pricing.lemma",
            ))),
        )
        .expect("default after unit should be valid");
    let now = DateTimeValue::now();

    let plan = engine.get_plan(None, "pricing", Some(&now)).unwrap();
    let schema = plan.schema(&lemma::DataOverlay::default());
    let entry = schema.data.get("money").expect("money data in schema");
    assert!(
        entry.lemma_type.is_quantity(),
        "money must be quantity money type"
    );
    assert_eq!(entry.lemma_type.name(), "money");
    match &entry.lemma_type.specifications {
        TypeSpecification::Quantity { units, .. } => {
            let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            assert!(names.contains(&"eur") && names.contains(&"usd"));
        }
        other => panic!("expected Quantity, got {:?}", other),
    }
    assert!(
        entry.default.is_some() && entry.bound_value.is_none(),
        "typedef money default must surface on price as schema suggestion"
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
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "ordering.lemma",
            ))),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan(None, "ordering", Some(&now)).unwrap();
    let schema = plan.schema(&lemma::DataOverlay::default());
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
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "ordering.lemma",
            ))),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan(None, "ordering", Some(&now)).unwrap();
    let schema = plan
        .schema_for_rules(&["total".to_string()], &lemma::DataOverlay::default())
        .unwrap();
    let data_names: Vec<&String> = schema.data.keys().collect();
    assert_eq!(
        data_names,
        vec!["zebra", "alpha", "middle"],
        "schema_for_rules should also preserve definition order"
    );
}

#[test]
fn test_schema_splits_bound_literal_and_default_suggestion() {
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
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "defaults.lemma",
            ))),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan(None, "defaults", Some(&now)).unwrap();
    let schema = plan.schema(&lemma::DataOverlay::default());

    let quantity = schema.data.get("quantity").expect("quantity should exist");
    assert!(
        quantity.default.is_some() && quantity.bound_value.is_none(),
        "type-level default is a suggestion only"
    );

    let name = schema.data.get("name").expect("name should exist");
    assert!(
        name.default.is_none() && name.bound_value.is_none(),
        "type-only data without default has no bound value or suggestion"
    );

    let price = schema.data.get("price").expect("price should exist");
    assert!(
        price.bound_value.is_some() && price.default.is_none(),
        "explicit literal is a bound value, not a default suggestion"
    );
}

#[test]
fn test_schema_quantity_default_is_value() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
        spec salary
        data money: quantity
          -> unit eur 1
          -> unit usd 0.84
          -> default 3000 eur
        data salary: money
        rule doubled: salary * 2
    "#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "salary.lemma",
            ))),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan(None, "salary", Some(&now)).unwrap();
    let schema = plan.schema(&lemma::DataOverlay::default());

    let salary = schema.data.get("salary").expect("salary should exist");
    assert!(
        salary.default.is_some() && salary.bound_value.is_none(),
        "quantity typedef default must surface as schema suggestion on salary"
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
            data money: quantity
              -> unit eur 1
              -> default 4 eur
            data price: money
            data final_price: price
            rule doubled: final_price * 2
            "#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("chain.lemma"))),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let schema = engine
        .get_plan(None, "chain", Some(&now))
        .unwrap()
        .schema(&lemma::DataOverlay::default());
    let final_price = schema
        .data
        .get("final_price")
        .expect("final_price should exist");
    assert!(
        final_price.default.is_some() && final_price.bound_value.is_none(),
        "typedef default declared on ancestor type must inherit as suggestion on leaf binding"
    );
}
