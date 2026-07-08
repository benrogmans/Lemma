//! Spec prefilled values (`data is_member: false`) must not cause
//! `collect_needed_data_paths` to treat unless arms as dead. Only caller overlay
//! may skip branch inputs. Covers schema, `schema_for_rules`, and `response.data`.

use lemma::DataOverlay;
use lemma::DataValueInput;
use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

const PRICING_SPEC: &str = r#"
spec pricing
data base_price: 100
data is_member: false
data quantity: number
rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 15%
  unless is_member then 20%
rule discount_amount: base_price * discount
rule discounted_price: base_price - discount_amount
rule vat: discounted_price * 21%
rule total: discounted_price + vat
"#;

const CHOOSER_SPEC: &str = r#"
spec chooser

data mode: text -> options "simple" "complex"
data simple_input: number
data complex_input_a: number
data complex_input_b: number

rule result: veto "pick mode"
  unless mode is "simple" then simple_input
  unless mode is "complex" then complex_input_a + complex_input_b
"#;

fn data_path_names(response: &lemma::Response) -> Vec<String> {
    response
        .data
        .iter()
        .flat_map(|group| group.data.iter())
        .map(|data| data.path.input_key())
        .collect()
}

#[test]
fn schema_includes_prefilled_value_in_live_unless() {
    let mut engine = Engine::new();
    engine
        .load(PRICING_SPEC, lemma::SourceType::Volatile)
        .expect("pricing spec must load");
    let now = DateTimeValue::now();
    let schema = engine
        .schema(None, "pricing", Some(&now))
        .expect("schema must succeed");

    assert!(
        schema.data.contains_key("is_member"),
        "is_member must appear even when spec prefills false: {:?}",
        schema.data.keys().collect::<Vec<_>>()
    );
    assert!(
        schema.data["is_member"].prefilled.is_some(),
        "is_member must carry prefilled from spec literal"
    );
}

#[test]
fn schema_includes_overridable_prefilled_value_in_live_unless() {
    let code = r#"
spec t
data base_price: 100
data quantity: number
rule discount: 0%
  unless base_price < 100 then 10%
rule total: quantity
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("spec must load");
    let now = DateTimeValue::now();
    let schema = engine
        .schema(None, "t", Some(&now))
        .expect("schema must succeed");

    assert!(
        schema.data.contains_key("base_price"),
        "base_price must appear when unless arm can become live via override: {:?}",
        schema.data.keys().collect::<Vec<_>>()
    );
    let entry = schema
        .data
        .get("base_price")
        .expect("base_price entry must exist");
    assert!(
        entry.prefilled.is_some(),
        "base_price must show spec prefilled literal"
    );
}

#[test]
fn schema_omits_input_when_unless_never_applies() {
    let code = r#"
spec t
data flag: boolean
rule discount: 0%
  unless flag and (1 > 2) then 20%
rule total: 1
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("spec must load");
    let now = DateTimeValue::now();
    let schema = engine
        .schema(None, "t", Some(&now))
        .expect("schema must succeed");

    assert!(
        !schema.data.contains_key("flag"),
        "flag must not appear when unless arm can never apply: {:?}",
        schema.data.keys().collect::<Vec<_>>()
    );
}

#[test]
fn schema_omits_dead_branch_inputs_when_mode_supplied() {
    let mut engine = Engine::new();
    engine
        .load(CHOOSER_SPEC, lemma::SourceType::Volatile)
        .expect("chooser spec must load");
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
    .expect("overlay must resolve");

    let schema = plan
        .schema_for_rules(&["result".to_string()], &overlay)
        .expect("schema must succeed");

    assert!(
        schema.data.contains_key("simple_input"),
        "simple_input must remain when mode is simple"
    );
    assert!(
        !schema.data.contains_key("complex_input_a"),
        "complex_input_a must be skipped when mode is simple"
    );
    assert!(
        !schema.data.contains_key("complex_input_b"),
        "complex_input_b must be skipped when mode is simple"
    );
}

#[test]
fn response_includes_prefilled_value_referenced_by_live_unless() {
    let mut engine = Engine::new();
    engine
        .load(PRICING_SPEC, lemma::SourceType::Volatile)
        .expect("pricing spec must load");
    let now = DateTimeValue::now();
    let mut inputs = HashMap::new();
    inputs.insert("quantity".to_string(), "5".to_string());

    let response = engine
        .run(None, "pricing", Some(&now), inputs, false, None)
        .expect("evaluation must succeed");

    let names = data_path_names(&response);
    assert!(
        names.contains(&"is_member".to_string()),
        "is_member must appear in response.data when unless arm may still apply: {names:?}"
    );
}

#[test]
fn eval_honors_supplied_override_for_unless_arm() {
    let mut engine = Engine::new();
    engine
        .load(PRICING_SPEC, lemma::SourceType::Volatile)
        .expect("pricing spec must load");
    let now = DateTimeValue::now();

    let default_schema = engine
        .schema(None, "pricing", Some(&now))
        .expect("schema must succeed");
    assert!(default_schema.data.contains_key("is_member"));

    let mut inputs = HashMap::new();
    inputs.insert("quantity".to_string(), "5".to_string());
    inputs.insert("is_member".to_string(), "true".to_string());

    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            inputs,
            false,
            Some(&["discount".to_string()]),
        )
        .expect("evaluation must succeed");

    let discount = response
        .results
        .get("discount")
        .expect("discount must be present");
    assert_eq!(
        discount.display.as_deref(),
        Some("20%"),
        "supplied override is_member true must activate member unless arm"
    );
}
