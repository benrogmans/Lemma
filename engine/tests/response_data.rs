//! Tests for `Response::data`: the effective values of data statically
//! referenced by the requested rules.
//!
//! `response.data` is populated from the plan's static walk
//! (`ExecutionPlan::collect_needed_data_paths`), not from what the compiled
//! instructions happened to read: a data path the virtual machine
//! skips at runtime is still listed, because the source rules reference it.

use lemma::{DateTimeValue, Engine};
use std::collections::HashMap;

fn data_path_names(response: &lemma::Response) -> Vec<String> {
    response
        .data
        .iter()
        .flat_map(|group| group.data.iter())
        .map(|data| data.path.input_key())
        .collect()
}

#[test]
fn response_data_lists_statically_referenced_data_for_requested_rules() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec demo
data expensive: 10
data threshold: number
data unrelated: 5
rule main: expensive * 2
  unless now > 1970-01-01 then threshold + 1
rule other: unrelated * 2
"#,
            lemma::SourceType::Volatile,
        )
        .expect("spec must load");

    let now = DateTimeValue::now();
    let mut inputs = HashMap::new();
    inputs.insert("threshold".to_string(), "4".to_string());
    let response = engine
        .run(
            None,
            "demo",
            Some(&now),
            inputs,
            false,
            Some(&["main".to_string()]),
        )
        .expect("evaluation must succeed");

    // The unless condition is true at runtime, so the virtual machine never
    // evaluates the default branch (`expensive * 2`). The condition depends on
    // `now`, so the branch is statically live and `expensive` stays listed.
    let names = data_path_names(&response);
    assert!(
        names.contains(&"threshold".to_string()),
        "threshold is referenced by the winning branch: {names:?}"
    );
    assert!(
        names.contains(&"expensive".to_string()),
        "expensive is statically referenced by the requested rule even though \
         the virtual machine skips the default branch at runtime: {names:?}"
    );
    assert!(
        !names.contains(&"unrelated".to_string()),
        "unrelated is only referenced by an unrequested rule: {names:?}"
    );
}

#[test]
fn response_data_excludes_data_without_an_effective_value() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec demo
data supplied: number
data missing: number
rule main: supplied + missing
"#,
            lemma::SourceType::Volatile,
        )
        .expect("spec must load");

    let now = DateTimeValue::now();
    let mut inputs = HashMap::new();
    inputs.insert("supplied".to_string(), "4".to_string());
    let response = engine
        .run(None, "demo", Some(&now), inputs, false, None)
        .expect("evaluation must succeed");

    let names = data_path_names(&response);
    assert!(
        names.contains(&"supplied".to_string()),
        "supplied has an effective value: {names:?}"
    );
    assert!(
        !names.contains(&"missing".to_string()),
        "missing has no effective value and surfaces through the missing-data \
         veto instead: {names:?}"
    );
}

#[test]
fn response_data_includes_data_reached_through_rule_target_reference() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec inner
data slot: number

spec source_spec
data v: 5
rule computed: v * 2

spec outer
uses i: inner
uses src: source_spec
with i.slot: src.computed
rule r: i.slot + 1
"#,
            lemma::SourceType::Volatile,
        )
        .expect("spec must load");

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "outer", Some(&now), HashMap::new(), false, None)
        .expect("evaluation must succeed");

    let names = data_path_names(&response);
    assert!(
        names.contains(&"src.v".to_string()),
        "the walk must follow the rule-target reference 'i.slot' to rule \
         'src.computed' and list its data 'src.v': {names:?}"
    );
}
