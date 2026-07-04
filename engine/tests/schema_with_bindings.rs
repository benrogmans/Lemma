//! Schema must not list `with`-bound data (`DataDefinition::Reference`).

use lemma::DataOverlay;
use lemma::DataValueInput;
use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

#[test]
fn schema_omits_with_bound_rule_references() {
    let code = r#"
spec bag
uses lemma units

data weight: measure
  -> unit kg 1

data money: measure
  -> unit eur 1

data price_per_weight: measure
  -> unit eur_per_kg eur/kg

data item_cost: price_per_weight
data roasting: price_per_weight
data chocolatizing: price_per_weight

rule total_price: weight * (item_cost + roasting + chocolatizing)

spec calc
uses bag
with bag.item_cost: item_cost
with bag.roasting: roasting

data type_of_nut: text -> options "peanut" "cashew"

rule price_peanut: 1.5 eur_per_kg
rule price_peanut_roasting: 0.45 eur_per_kg

rule price_cashew: 2.0 eur_per_kg
rule price_cashew_roasting: 0.55 eur_per_kg

rule item_cost: veto "No item cost"
  unless type_of_nut is "peanut" then price_peanut
  unless type_of_nut is "cashew" then price_cashew

rule roasting: veto "No roasting"
  unless type_of_nut is "peanut" then price_peanut_roasting
  unless type_of_nut is "cashew" then price_cashew_roasting

rule total_price: bag.total_price
"#;

    let mut engine = Engine::new();
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "schema_with_bindings.lemma",
            ))),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "calc", Some(&now)).unwrap();
    let schema = plan
        .schema_for_rules(&["total_price".to_string()], &DataOverlay::default())
        .unwrap();

    assert!(
        !schema.data.contains_key("bag.item_cost"),
        "with-bound rule ref must not appear in schema: {:?}",
        schema.data.keys().collect::<Vec<_>>()
    );
    assert!(
        !schema.data.contains_key("bag.roasting"),
        "with-bound rule ref must not appear in schema"
    );
    assert!(
        schema.data.contains_key("bag.chocolatizing"),
        "unbound nested data must still appear"
    );
    assert!(
        schema.data.contains_key("bag.weight"),
        "nested input data must appear"
    );
    assert!(
        schema.data.contains_key("type_of_nut"),
        "local input for bound rules must appear"
    );

    let data_names: Vec<&String> = schema.data.keys().collect();
    assert_eq!(
        data_names,
        vec!["type_of_nut", "bag.weight", "bag.chocolatizing"],
        "local data must precede nested data, each group in definition order"
    );
}

#[test]
fn schema_lists_literal_with_binding_as_bound_not_promptable() {
    let code = r#"
spec inner
data v: number

spec outer
uses i: inner
with i.v: 42
rule r: i.v
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let schema = engine
        .get_plan(None, "outer", Some(&now))
        .unwrap()
        .schema(&DataOverlay::default());
    let entry = schema
        .data
        .get("i.v")
        .expect("literal with still surfaces in schema for documentation");

    assert!(
        entry.bound_value.is_some(),
        "literal with is bound_value; CLI skips, not a free input"
    );
}

#[test]
fn schema_omits_with_bound_data_reference() {
    let code = r#"
spec inner
data a: number -> default 1
data b: number

spec outer
uses i: inner
with i.a: i.b
data input: number
rule r: i.a + input
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let schema = engine
        .get_plan(None, "outer", Some(&now))
        .unwrap()
        .schema_for_rules(&["r".to_string()], &DataOverlay::default())
        .unwrap();

    assert!(!schema.data.contains_key("i.a"));
    assert!(schema.data.contains_key("input"));
}

#[test]
fn run_still_resolves_with_bound_fields_without_schema_inputs() {
    let code = r#"
spec inner
data v: number

spec outer
uses i: inner
with i.v: 10
rule r: i.v
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let resp = engine
        .run(None, "outer", Some(&now), HashMap::new(), false, None)
        .unwrap();
    let r = resp.results.values().find(|x| x.rule.name == "r").unwrap();
    assert_eq!(r.display.as_deref(), Some("10"));
}

/// Spec that has genuinely different data requirements per branch — used to
/// verify that partial evaluation prunes the unreachable data.
const CHOOSER_LEMMA: &str = r#"
spec chooser

data mode: text -> options "simple" "complex"
data simple_input: number
data complex_input_a: number
data complex_input_b: number

rule result: veto "pick mode"
  unless mode is "simple" then simple_input
  unless mode is "complex" then complex_input_a + complex_input_b
"#;

#[test]
fn schema_for_rules_returns_all_data_on_fresh_plan() {
    let mut engine = Engine::new();
    engine
        .load(CHOOSER_LEMMA, lemma::SourceType::Volatile)
        .unwrap();
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "chooser", Some(&now)).unwrap();
    let schema = plan
        .schema_for_rules(&["result".to_string()], &DataOverlay::default())
        .unwrap();

    let names: Vec<&String> = schema.data.keys().collect();
    assert_eq!(
        names,
        vec!["mode", "simple_input", "complex_input_a", "complex_input_b"],
        "fresh plan should expose all data in source order"
    );
}

#[test]
fn schema_for_rules_prunes_complex_branch_when_mode_is_simple() {
    let mut engine = Engine::new();
    engine
        .load(CHOOSER_LEMMA, lemma::SourceType::Volatile)
        .unwrap();
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "chooser", Some(&now)).unwrap();

    let overlay = DataOverlay::resolve(
        plan,
        [(
            "mode".to_string(),
            DataValueInput::convenience("simple".to_string()),
        )]
        .into(),
        engine.limits(),
    )
    .unwrap();

    let schema = plan
        .schema_for_rules(&["result".to_string()], &overlay)
        .unwrap();

    assert!(
        schema.data.contains_key("simple_input"),
        "simple_input must remain when mode is simple"
    );
    assert!(
        !schema.data.contains_key("complex_input_a"),
        "complex_input_a must be pruned when mode is simple"
    );
    assert!(
        !schema.data.contains_key("complex_input_b"),
        "complex_input_b must be pruned when mode is simple"
    );
}

#[test]
fn schema_for_rules_prunes_simple_branch_when_mode_is_complex() {
    let mut engine = Engine::new();
    engine
        .load(CHOOSER_LEMMA, lemma::SourceType::Volatile)
        .unwrap();
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "chooser", Some(&now)).unwrap();

    let overlay = DataOverlay::resolve(
        plan,
        [(
            "mode".to_string(),
            DataValueInput::convenience("complex".to_string()),
        )]
        .into(),
        engine.limits(),
    )
    .unwrap();

    let schema = plan
        .schema_for_rules(&["result".to_string()], &overlay)
        .unwrap();

    assert!(
        !schema.data.contains_key("simple_input"),
        "simple_input must be pruned when mode is complex"
    );
    assert!(
        schema.data.contains_key("complex_input_a"),
        "complex_input_a must remain when mode is complex"
    );
    assert!(
        schema.data.contains_key("complex_input_b"),
        "complex_input_b must remain when mode is complex"
    );
}

#[test]
fn calc_schema_bound_type_of_nut_prunes_inactive_nut_branches() {
    let code = r#"
spec bag
uses lemma units

data weight: measure
  -> unit kg 1

data money: measure
  -> unit eur 1

data price_per_weight: measure
  -> unit eur_per_kg eur/kg

data item_cost: price_per_weight
data roasting: price_per_weight
data chocolatizing: price_per_weight

rule total_price: weight * (item_cost + roasting + chocolatizing)

spec calc
uses bag
with bag.item_cost: item_cost
with bag.roasting: roasting

data type_of_nut: text -> options "peanut" "cashew"

rule price_peanut: 1.5 eur_per_kg
rule price_peanut_roasting: 0.45 eur_per_kg

rule price_cashew: 2.0 eur_per_kg
rule price_cashew_roasting: 0.55 eur_per_kg

rule item_cost: veto "No item cost"
  unless type_of_nut is "peanut" then price_peanut
  unless type_of_nut is "cashew" then price_cashew

rule roasting: veto "No roasting"
  unless type_of_nut is "peanut" then price_peanut_roasting
  unless type_of_nut is "cashew" then price_cashew_roasting

rule total_price: bag.total_price
"#;

    let mut engine = Engine::new();
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("calc.lemma"))),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "calc", Some(&now)).unwrap();

    let overlay = DataOverlay::resolve(
        plan,
        [(
            "type_of_nut".to_string(),
            DataValueInput::convenience("peanut".to_string()),
        )]
        .into(),
        engine.limits(),
    )
    .unwrap();

    let schema = plan
        .schema_for_rules(&["total_price".to_string()], &overlay)
        .unwrap();

    let type_of_nut_entry = schema
        .data
        .get("type_of_nut")
        .expect("type_of_nut must be in schema");
    assert!(
        type_of_nut_entry.bound_value.is_some(),
        "type_of_nut must be bound after providing it"
    );

    assert!(
        schema.data.contains_key("bag.weight"),
        "bag.weight must remain"
    );
    assert!(
        schema.data.contains_key("bag.chocolatizing"),
        "bag.chocolatizing must remain"
    );

    assert!(!schema.data.contains_key("bag.item_cost"));
    assert!(!schema.data.contains_key("bag.roasting"));
}
