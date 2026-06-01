//! Invalid runtime data overrides complete evaluation with Veto, not abort with Error.

use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn coffee_example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../documentation/examples/01_coffee_order.lemma")
}

fn recipe_example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../documentation/examples/03_recipe_scaling.lemma")
}

fn load_coffee(engine: &mut Engine) {
    let code = std::fs::read_to_string(coffee_example_path()).expect("read coffee example");
    engine
        .load(
            &code,
            lemma::SourceType::Path(Arc::new(PathBuf::from("01_coffee_order.lemma"))),
        )
        .expect("load coffee_order");
}

fn load_recipe(engine: &mut Engine) {
    let code = std::fs::read_to_string(recipe_example_path()).expect("read recipe example");
    engine
        .load(
            &code,
            lemma::SourceType::Path(Arc::new(PathBuf::from("03_recipe_scaling.lemma"))),
        )
        .expect("load recipe_scaling");
}

fn full_coffee_data(product: &str) -> HashMap<String, String> {
    HashMap::from([
        ("product".to_string(), product.to_string()),
        ("size".to_string(), "medium".to_string()),
        ("number_of_cups".to_string(), "1".to_string()),
        ("has_loyalty_card".to_string(), "false".to_string()),
        ("age".to_string(), "30".to_string()),
    ])
}

fn assert_run_completes_with_veto_not_validation_error(
    result: Result<lemma::Response, lemma::Error>,
    context: &str,
) -> lemma::Response {
    match result {
        Ok(response) => response,
        Err(err) => {
            panic!("{context}: run must complete with veto, not abort with Error — got: {err}")
        }
    }
}

#[test]
fn invalid_text_option_override_completes_with_veto_not_validation_error() {
    let mut engine = Engine::new();
    load_coffee(&mut engine);

    let now = DateTimeValue::now();
    let data = full_coffee_data("tea");

    let response = assert_run_completes_with_veto_not_validation_error(
        engine.run(None, "coffee_order", Some(&now), data, false),
        "product=tea (not in options)",
    );

    let base = response
        .results
        .get("base_price")
        .expect("base_price in results");
    assert!(
        base.vetoed,
        "invalid product override must veto base_price, not fail run"
    );
}

#[test]
fn below_minimum_number_override_completes_with_veto_not_validation_error() {
    let mut engine = Engine::new();
    load_coffee(&mut engine);

    let now = DateTimeValue::now();
    let mut data = full_coffee_data("latte");
    data.insert("age".to_string(), "-5".to_string());

    let response = assert_run_completes_with_veto_not_validation_error(
        engine.run(None, "coffee_order", Some(&now), data, false),
        "age=-5 (below minimum 0)",
    );

    assert!(
        response.results.values().any(|r| r.vetoed),
        "below-minimum age must produce at least one vetoed rule"
    );
}

#[test]
fn unparsable_number_override_completes_with_veto_not_validation_error() {
    let code = r#"
spec s
data age: number
rule doubled: age * 2
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("plan");

    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("age".to_string(), "twenty".to_string());

    let response = assert_run_completes_with_veto_not_validation_error(
        engine.run(None, "s", Some(&now), data, false),
        "age=twenty (not a number)",
    );

    let doubled = response.results.get("doubled").expect("doubled");
    assert!(doubled.vetoed, "unparsable age must veto doubled");
}

#[test]
fn below_minimum_typedecl_override_completes_with_veto_not_validation_error() {
    let mut engine = Engine::new();
    load_recipe(&mut engine);

    let now = DateTimeValue::now();
    let data = HashMap::from([
        ("desired_servings".to_string(), "0".to_string()),
        ("original_servings".to_string(), "4".to_string()),
        ("recipe_name".to_string(), "chocolate_cake".to_string()),
    ]);

    let response = assert_run_completes_with_veto_not_validation_error(
        engine.run(None, "recipe_scaling", Some(&now), data, false),
        "desired_servings=0 (below minimum 1)",
    );

    assert!(
        response.results.values().any(|r| r.vetoed),
        "below-minimum desired_servings must produce veto on dependent rules"
    );
}

#[test]
fn invalid_boolean_override_completes_with_veto_not_validation_error() {
    let code = r#"
spec s
data active: boolean
rule flag: active
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("plan");

    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("active".to_string(), "maybe".to_string());

    let response = assert_run_completes_with_veto_not_validation_error(
        engine.run(None, "s", Some(&now), data, false),
        "active=maybe (not boolean)",
    );

    let flag = response.results.get("flag").expect("flag");
    assert!(flag.vetoed, "invalid boolean override must veto flag");
}
