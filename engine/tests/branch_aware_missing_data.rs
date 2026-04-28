//! `Response::missing_data` / `missing_data_ordered` from `VetoType::MissingData` (evaluation order).
//!
//! Note: rules are still evaluated in topological order for the whole plan, so a rule like
//! `size_multiplier` may veto before a later rule's `unless` avoids using it. True branch-only
//! skipping for prompting needs lazy/on-demand rule evaluation (not implemented here).

use lemma::parsing::ast::DateTimeValue;
use lemma::planning::semantics::DataPath;
use lemma::Engine;
use std::collections::HashMap;
use std::path::PathBuf;

fn coffee_example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../documentation/examples/01_coffee_order.lemma")
}

#[test]
fn missing_data_ordered_empty_when_all_datas_provided() {
    let mut engine = Engine::new();
    let path = coffee_example_path();
    let code = std::fs::read_to_string(&path).expect("read example");
    engine
        .load(&code, lemma::SourceType::Labeled("01_coffee_order.lemma"))
        .expect("load");

    let now = DateTimeValue::now();
    let plan = engine.get_plan("coffee_order", Some(&now)).expect("plan");

    let mut data = HashMap::new();
    data.insert("product".to_string(), "latte".to_string());
    data.insert("size".to_string(), "medium".to_string());
    data.insert("number_of_cups".to_string(), "1".to_string());
    data.insert("has_loyalty_card".to_string(), "false".to_string());
    data.insert("age".to_string(), "30".to_string());

    let response = engine.run_plan(plan, Some(&now), data, false).expect("run");
    assert!(
        response.missing_data_ordered().is_empty(),
        "all data provided: {:?}",
        response.missing_data_ordered()
    );
}

#[test]
fn missing_data_ordered_includes_product_when_no_inputs() {
    let mut engine = Engine::new();
    let path = coffee_example_path();
    let code = std::fs::read_to_string(&path).expect("read example");
    engine
        .load(&code, lemma::SourceType::Labeled("01_coffee_order.lemma"))
        .expect("load");

    let now = DateTimeValue::now();
    let plan = engine.get_plan("coffee_order", Some(&now)).expect("plan");

    let response = engine
        .run_plan(plan, Some(&now), HashMap::new(), false)
        .expect("run");

    let ordered = response.missing_data_ordered();
    assert!(
        ordered.contains(&DataPath::local("product".to_string())),
        "expected product among missing data, got {:?}",
        ordered
    );
    assert_eq!(
        ordered.len(),
        response.missing_data().len(),
        "set vs ordered length"
    );
}
