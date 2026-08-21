//! Behaviour of unless chains that planning folds into an ordered dispatch table.
//!
//! The fold is invisible by design: these cases pin the observable behaviour it must
//! not change — selected result, veto propagation, released data and narration.
//! Plan-shape assertions live in `engine/src/tests/transitive_normalization_plan_shape.rs`.

use lemma::{DateTimeValue, Engine, Response, RuleResult};
use std::collections::HashMap;

fn engine_for(code: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .expect("spec must load");
    engine
}

fn run(engine: &Engine, spec: &str, bindings: &[(&str, &str)], explain: bool) -> Response {
    let now = DateTimeValue::now();
    let data: HashMap<String, String> = bindings
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect();
    engine
        .run(None, spec, Some(&now), data, None, explain)
        .expect("evaluation must succeed")
}

fn rule<'a>(response: &'a Response, name: &str) -> &'a RuleResult {
    response.results.get(name).unwrap_or_else(|| {
        panic!(
            "rule '{name}' missing from results: {:?}",
            response.results.keys().collect::<Vec<_>>()
        )
    })
}

fn display(response: &Response, name: &str) -> String {
    rule(response, name)
        .display()
        .map(|d| d.to_string())
        .unwrap_or_else(|| {
            panic!(
                "rule '{name}' produced no value: {:?}",
                rule(response, name)
            )
        })
}

const LOOKUP: &str = r#"
spec lookup
data code: text
  -> option "NL"
  -> option "BE"
  -> option "DE"
rule name: veto "unknown code"
  unless code is "NL" then "Netherlands"
  unless code is "BE" then "Belgium"
  unless code is "DE" then "Germany"
"#;

const TIERS: &str = r#"
spec tiers
data quantity: number
rule discount: 0
  unless quantity >= 10 then 5
  unless quantity >= 100 then 10
  unless quantity is 50 then 7
"#;

#[test]
fn equality_lookup_selects_the_matching_arm() {
    let engine = engine_for(LOOKUP);
    for (code, expected) in [("NL", "Netherlands"), ("BE", "Belgium"), ("DE", "Germany")] {
        assert_eq!(
            display(&run(&engine, "lookup", &[("code", code)], false), "name"),
            expected
        );
    }
}

#[test]
fn ordering_chain_holds_at_below_and_above_every_boundary() {
    let engine = engine_for(TIERS);
    // 50 is an exact hit on the last arm, which wins over the `>= 10` arm below it.
    for (quantity, expected) in [
        ("0", "0"),
        ("9", "0"),
        ("10", "5"),
        ("49", "5"),
        ("50", "7"),
        ("51", "5"),
        ("99", "5"),
        ("100", "10"),
        ("1000", "10"),
    ] {
        assert_eq!(
            display(
                &run(&engine, "tiers", &[("quantity", quantity)], false),
                "discount"
            ),
            expected,
            "quantity {quantity}"
        );
    }
}

#[test]
fn a_negative_scrutinee_lands_below_every_boundary() {
    let engine = engine_for(TIERS);
    assert_eq!(
        display(
            &run(&engine, "tiers", &[("quantity", "-1000")], false),
            "discount"
        ),
        "0"
    );
}

#[test]
fn an_unmatched_scrutinee_falls_through_to_the_default() {
    let engine = engine_for(
        r#"
spec lookup
data code: text
rule name: veto "unknown code"
  unless code is "NL" then "Netherlands"
"#,
    );
    let response = run(&engine, "lookup", &[("code", "ZZ")], false);
    assert!(
        rule(&response, "name").vetoed,
        "an unmatched code must reach the default veto: {:?}",
        rule(&response, "name")
    );
}

#[test]
fn an_unbound_scrutinee_propagates_as_missing_data() {
    let engine = engine_for(LOOKUP);
    let response = run(&engine, "lookup", &[], false);
    let result = rule(&response, "name");
    assert!(
        result.vetoed,
        "an unbound scrutinee must veto, got {result:?}"
    );
    assert_eq!(
        result.missing_data(),
        vec!["code".to_string()],
        "the scrutinee is the only thing the rule still needs"
    );
}

/// The regions the dispatch did not select must release their data, exactly as the
/// untaken arms of a Piecewise do.
#[test]
fn losing_regions_release_their_data() {
    let engine = engine_for(
        r#"
spec routing
data code: text
data dutch_rate: number
data belgian_rate: number
rule rate: 0
  unless code is "NL" then dutch_rate
  unless code is "BE" then belgian_rate
"#,
    );
    let response = run(&engine, "routing", &[("code", "NL")], false);
    let missing = rule(&response, "rate").missing_data();
    assert_eq!(
        missing,
        ["dutch_rate".to_string()].as_slice(),
        "only the selected region's data is still needed, got {missing:?}"
    );
}

#[test]
fn every_region_is_released_when_the_default_wins() {
    let engine = engine_for(
        r#"
spec routing
data code: text
data dutch_rate: number
data belgian_rate: number
rule rate: 0
  unless code is "NL" then dutch_rate
  unless code is "BE" then belgian_rate
"#,
    );
    let response = run(&engine, "routing", &[("code", "ZZ")], false);
    assert!(
        rule(&response, "rate").missing_data().is_empty(),
        "no arm can win, so neither rate is needed: {:?}",
        rule(&response, "rate").missing_data()
    );
}

/// A result shared by several regions stays live when one of them is selected.
#[test]
fn a_result_reachable_from_two_regions_is_not_released() {
    let engine = engine_for(
        r#"
spec shared
data code: text
data special: number
rule rate: 0
  unless code is "NL" then special
  unless code is "BE" then special
"#,
    );
    let response = run(&engine, "shared", &[("code", "BE")], false);
    assert_eq!(
        rule(&response, "rate").missing_data(),
        vec!["special".to_string()],
        "the selected region needs it, so it must not be released"
    );
}

#[test]
fn explain_and_value_modes_agree_at_every_tier_region() {
    let engine = engine_for(TIERS);
    for quantity in ["0", "9", "10", "49", "50", "51", "99", "100", "1000"] {
        let without = display(
            &run(&engine, "tiers", &[("quantity", quantity)], false),
            "discount",
        );
        let with = display(
            &run(&engine, "tiers", &[("quantity", quantity)], true),
            "discount",
        );
        assert_eq!(
            without, with,
            "quantity {quantity}: explain result must match value mode"
        );
    }
}

#[test]
fn explanation_states_the_matched_condition_with_its_data() {
    let engine = engine_for(TIERS);
    let response = run(&engine, "tiers", &[("quantity", "100")], true);
    let explanation = rule(&response, "discount")
        .explanation
        .as_ref()
        .expect("explanation built");

    let matched = explanation
        .causes
        .iter()
        .find(|cause| cause.condition == "quantity >= 100")
        .unwrap_or_else(|| panic!("matched condition missing from {:?}", explanation.causes));
    assert_eq!(matched.value, "true");
    assert!(
        matched
            .children
            .iter()
            .any(|child| format!("{child:?}").contains("100")),
        "the cause must carry the value that drove it, got {:?}",
        matched.children
    );
}

#[test]
fn explanation_states_untaken_conditions_as_the_flipped_fact() {
    let engine = engine_for(TIERS);
    let response = run(&engine, "tiers", &[("quantity", "10")], true);
    let explanation = rule(&response, "discount")
        .explanation
        .as_ref()
        .expect("explanation built");
    let conditions: Vec<&str> = explanation
        .causes
        .iter()
        .map(|cause| cause.condition.as_str())
        .collect();
    assert_eq!(
        conditions,
        vec!["quantity >= 10"],
        "only the winning arm is a cause once the later arms are false"
    );
}

/// Narration walks the pre-image chain, so a vetoed scrutinee still reports the
/// condition it was evaluating — the dispatch table has no conditions to name.
#[test]
fn a_vetoed_scrutinee_narrates_a_condition_from_the_pre_image() {
    let engine = engine_for(LOOKUP);
    let response = run(&engine, "lookup", &[], true);
    let explanation = rule(&response, "name")
        .explanation
        .as_ref()
        .expect("explanation built");
    assert_eq!(
        explanation.body, "code is DE",
        "the reverse scan starts at the last arm and propagates its veto"
    );
}
