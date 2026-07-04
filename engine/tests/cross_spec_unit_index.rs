//! Cross-spec plans must execute dependency unit conversions without widening
//! the consumer spec's expression-scope unit index.

use lemma::{DateTimeValue, Engine, ExecutionPlan, SourceType};
use std::collections::HashMap;

const UNITS_SPEC: &str = r#"
spec units
uses lemma units
data money: measure
  -> unit eur 1
  -> decimals 2
"#;

const WAREHOUSING_SPEC: &str = r#"
spec warehousing
uses units
uses si: lemma units

data units_per_pallet: number
  -> minimum 1
  -> default 1

data storage_duration: si.duration
  -> minimum 0 weeks
  -> default 10 days

data interbranch_transport_per_pallet: units.money
  -> minimum 0 eur
  -> default 0 eur

data inbound_handling_per_pallet: units.money
  -> minimum 0 eur
  -> default 0 eur

data storage_per_pallet_per_week: units.money
  -> minimum 0 eur
  -> default 10 eur

data labeling_per_pallet: units.money
  -> minimum 0 eur
  -> default 0 eur

data outbound_handling_per_pallet: units.money
  -> minimum 0 eur
  -> default 0 eur

rule storage_cost_per_pallet:
  storage_per_pallet_per_week
  * ceil storage_duration as weeks as Number

rule total_logistics_per_pallet:
  interbranch_transport_per_pallet
  + inbound_handling_per_pallet
  + storage_cost_per_pallet
  + labeling_per_pallet
  + outbound_handling_per_pallet

rule total_logistics_per_ce:
  total_logistics_per_pallet / units_per_pallet
"#;

const QUOTATION_SPEC: &str = r#"
spec quotation
uses wh: warehousing
rule total: wh.total_logistics_per_ce
"#;

const QUOTATION_BAD_SPEC: &str = r#"
spec quotation_bad
uses wh: warehousing
rule bad: 5 minutes
"#;

fn load_specs(engine: &mut Engine) {
    engine
        .load(UNITS_SPEC, SourceType::Volatile)
        .expect("units spec must load");
    engine
        .load(WAREHOUSING_SPEC, SourceType::Volatile)
        .expect("warehousing spec must load");
}

#[test]
fn warehousing_plans_alone() {
    let mut engine = Engine::new();
    load_specs(&mut engine);
    let now = DateTimeValue::now();
    engine
        .get_plan(None, "warehousing", Some(&now))
        .expect("warehousing must plan alone");
    let response = engine
        .run(None, "warehousing", Some(&now), HashMap::new(), false, None)
        .expect("warehousing must evaluate");
    let display = response
        .results
        .get("storage_cost_per_pallet")
        .expect("storage_cost_per_pallet must be present")
        .display
        .clone()
        .expect("storage_cost_per_pallet must have display");
    assert_eq!(
        display, "20.00 eur",
        "10 eur/week * ceil(10 days as weeks) must be 20.00 eur, got: {display}"
    );
}

#[test]
fn quotation_plans_without_consumer_stdlib_units() {
    let mut engine = Engine::new();
    load_specs(&mut engine);
    engine
        .load(QUOTATION_SPEC, SourceType::Volatile)
        .expect("quotation must plan without uses lemma units");
    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "quotation", Some(&now))
        .expect("quotation must plan without Unknown unit minutes/weeks in unit index");

    let expression_units = &plan.resolved_types.unit_index;
    assert!(
        !expression_units.contains_key("weeks"),
        "consumer expression scope must not contain weeks: {:?}",
        expression_units.keys().collect::<Vec<_>>()
    );
    assert!(
        !expression_units.contains_key("minutes"),
        "consumer expression scope must not contain minutes: {:?}",
        expression_units.keys().collect::<Vec<_>>()
    );
    let mut keys: Vec<_> = expression_units.keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["percent", "permille"],
        "consumer expression scope must only have builtin ratio units, not dependency units"
    );
}

#[test]
fn quotation_evaluates_cross_spec_duration_conversion() {
    let mut engine = Engine::new();
    load_specs(&mut engine);
    engine
        .load(QUOTATION_SPEC, SourceType::Volatile)
        .expect("quotation must load");
    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "quotation", Some(&now))
        .expect("quotation must plan");
    assert!(
        !plan.resolved_types.unit_index.contains_key("weeks"),
        "consumer unit_index must not contain weeks before serialize"
    );

    let mut json: serde_json::Value =
        serde_json::to_value(lemma::ExecutionPlanSerialized::from(plan)).unwrap();
    let unit_index = json["unit_index"]
        .as_object_mut()
        .expect("serialized plan must have unit_index");
    unit_index.remove("weeks");
    unit_index.remove("minutes");

    let serialized: lemma::ExecutionPlanSerialized = serde_json::from_value(json).unwrap();
    let reconstructed = ExecutionPlan::try_from(serialized)
        .expect("inlined weeks conversion must validate via owning_type, not consumer unit_index");
    let response = engine
        .run_plan(&reconstructed, Some(&now), HashMap::new(), false, None)
        .expect("serialized quotation plan must evaluate");
    let display = response
        .results
        .get("total")
        .expect("rule total must be present")
        .display
        .clone()
        .expect("total must have display");
    assert_eq!(
        display, "20.00 eur",
        "10 eur/week * ceil(10 days as weeks) / 1 CE must be 20.00 eur, got: {display}"
    );
}

#[test]
fn quotation_rejects_minutes_in_local_rule() {
    let mut engine = Engine::new();
    load_specs(&mut engine);
    let err = engine
        .load(QUOTATION_BAD_SPEC, SourceType::Volatile)
        .expect_err("5 minutes in consumer must fail at load");
    let minutes_err = err
        .errors
        .iter()
        .find(|error| error.message().contains("minutes"))
        .expect("load must report minutes out of scope");
    assert_eq!(
        minutes_err.message(),
        "Unit 'minutes' is not in scope for this spec"
    );
}
