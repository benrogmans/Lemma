//! Red TDD: per-rule `RuleResult.missing_data` from `Engine::run`.
//!
//! Asserts on Rust `Response` / `RuleResult` structures (not JSON).
//! Until implementation fills `missing_data`, cases that expect non-empty
//! lists fail asserts.

use lemma::{DateTimeValue, Engine, RuleResult};
use std::collections::HashMap;
use std::sync::Arc;

fn load(engine: &mut Engine, code: &str) {
    engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .expect("spec must load");
}

fn run(
    engine: &Engine,
    spec: &str,
    data: HashMap<String, String>,
    rules: Option<&[String]>,
    explain: bool,
) -> lemma::Response {
    let now = DateTimeValue::now();
    engine
        .run(None, spec, Some(&now), data, rules, explain)
        .expect("evaluation must succeed")
}

fn rule<'a>(response: &'a lemma::Response, name: &str) -> &'a RuleResult {
    response.results.get(name).unwrap_or_else(|| {
        panic!(
            "rule '{name}' missing from results: {:?}",
            response.results.keys().collect::<Vec<_>>()
        )
    })
}

fn missing<'a>(response: &'a lemma::Response, name: &str) -> &'a [String] {
    rule(response, name).missing_data.as_slice()
}

fn show_has(engine: &Engine, spec: &str, key: &str) {
    let now = DateTimeValue::now();
    let show = engine.show(None, spec, Some(&now)).expect("show");
    assert!(
        show.data.contains_key(key),
        "show.data must contain {key:?}; keys={:?}",
        show.data.keys().collect::<Vec<_>>()
    );
}

// --- A. Complete vs incomplete ---

#[test]
fn a1_full_overlay_all_missing_data_empty() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data a: number
data b: number
rule main: a + b
"#,
    );
    let mut data = HashMap::new();
    data.insert("a".to_string(), "1".to_string());
    data.insert("b".to_string(), "2".to_string());
    let response = run(&engine, "demo", data, None, false);
    let main = rule(&response, "main");
    assert!(
        !main.vetoed,
        "must not missing-data veto: {:?}",
        main.veto_reason
    );
    assert!(
        main.missing_data.is_empty(),
        "expected empty missing_data, got {:?}",
        main.missing_data
    );
}

#[test]
fn a2_no_inputs_lists_all_operands_in_declaration_order() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data a: number
data b: number
data c: number
rule main: a + b + c
"#,
    );
    let response = run(&engine, "demo", HashMap::new(), None, false);
    assert_eq!(missing(&response, "main"), &["a", "b", "c"]);
}

#[test]
fn a3_partial_overlay_omits_supplied_keys() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data a: number
data b: number
data c: number
rule main: a + b + c
"#,
    );
    let mut data = HashMap::new();
    data.insert("a".to_string(), "1".to_string());
    let response = run(&engine, "demo", data, None, false);
    assert_eq!(missing(&response, "main"), &["b", "c"]);
}

#[test]
fn a4_prefill_absent_suggest_present_in_missing_data() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data prefilled: 10
data suggested: number -> suggest 5
data required: number
rule main: prefilled + suggested + required
"#,
    );
    let response = run(&engine, "demo", HashMap::new(), None, false);
    let md = missing(&response, "main");
    assert!(
        !md.contains(&"prefilled".to_string()),
        "prefill must not be missing: {md:?}"
    );
    assert!(
        md.contains(&"suggested".to_string()),
        "suggest unbound must be missing: {md:?}"
    );
    assert!(
        md.contains(&"required".to_string()),
        "required unbound must be missing: {md:?}"
    );
}

// --- B. Per-rule scoping ---

#[test]
fn b1_two_rules_do_not_cross_leak_missing_data() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data x: number
data y: number
rule r1: x
rule r2: y
"#,
    );
    let response = run(
        &engine,
        "demo",
        HashMap::new(),
        Some(&["r1".to_string(), "r2".to_string()]),
        false,
    );
    assert_eq!(missing(&response, "r1"), &["x"]);
    assert_eq!(missing(&response, "r2"), &["y"]);
}

#[test]
fn b2_request_only_r1_excludes_y_from_results() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data x: number
data y: number
rule r1: x
rule r2: y
"#,
    );
    let response = run(
        &engine,
        "demo",
        HashMap::new(),
        Some(&["r1".to_string()]),
        false,
    );
    assert!(response.results.contains_key("r1"), "r1 must be present");
    assert!(
        !response.results.contains_key("r2"),
        "r2 must not be in results when unrequested"
    );
    assert_eq!(missing(&response, "r1"), &["x"]);
}

#[test]
fn b3_shared_unbound_input_listed_on_both_rules() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data s: number
data t: number
rule r1: s + 1
rule r2: s + t
"#,
    );
    let response = run(
        &engine,
        "demo",
        HashMap::new(),
        Some(&["r1".to_string(), "r2".to_string()]),
        false,
    );
    assert!(
        missing(&response, "r1").contains(&"s".to_string()),
        "r1 must list shared s: {:?}",
        missing(&response, "r1")
    );
    assert!(
        missing(&response, "r2").contains(&"s".to_string()),
        "r2 must list shared s (memo replay): {:?}",
        missing(&response, "r2")
    );
    assert!(
        missing(&response, "r2").contains(&"t".to_string()),
        "r2 must list t: {:?}",
        missing(&response, "r2")
    );

    let mut data = HashMap::new();
    data.insert("s".to_string(), "3".to_string());
    let response = run(
        &engine,
        "demo",
        data,
        Some(&["r1".to_string(), "r2".to_string()]),
        false,
    );
    assert!(
        !missing(&response, "r1").contains(&"s".to_string()),
        "r1 must not list supplied s: {:?}",
        missing(&response, "r1")
    );
    assert!(
        !missing(&response, "r2").contains(&"s".to_string()),
        "r2 must not list supplied s: {:?}",
        missing(&response, "r2")
    );
    assert_eq!(missing(&response, "r2"), &["t"]);
}

#[test]
fn b4_only_incomplete_sibling_has_non_empty_missing_data() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data ready: 1
data need: number
rule ok: ready
rule incomplete: need
"#,
    );
    let response = run(&engine, "demo", HashMap::new(), None, false);
    assert!(
        missing(&response, "ok").is_empty(),
        "ok must have empty missing_data: {:?}",
        missing(&response, "ok")
    );
    assert_eq!(missing(&response, "incomplete"), &["need"]);
}

// --- C. Unless / piecewise ---

const CHOOSER: &str = r#"
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
fn c1_mode_unbound_lists_mode_and_live_arm_inputs() {
    let mut engine = Engine::new();
    load(&mut engine, CHOOSER);
    let response = run(
        &engine,
        "chooser",
        HashMap::new(),
        Some(&["result".to_string()]),
        false,
    );
    let md = missing(&response, "result");
    assert!(md.contains(&"mode".to_string()), "mode: {md:?}");
    assert!(
        md.contains(&"simple_input".to_string()),
        "default/simple live: {md:?}"
    );
    assert!(
        md.contains(&"complex_input_a".to_string()),
        "complex arm live while mode unbound: {md:?}"
    );
    assert!(
        md.contains(&"complex_input_b".to_string()),
        "complex arm live while mode unbound: {md:?}"
    );
}

#[test]
fn c2_mode_simple_prunes_complex_inputs() {
    let mut engine = Engine::new();
    load(&mut engine, CHOOSER);
    let mut data = HashMap::new();
    data.insert("mode".to_string(), "simple".to_string());
    let response = run(
        &engine,
        "chooser",
        data,
        Some(&["result".to_string()]),
        false,
    );
    let md = missing(&response, "result");
    assert!(
        !md.contains(&"complex_input_a".to_string()),
        "complex pruned: {md:?}"
    );
    assert!(
        !md.contains(&"complex_input_b".to_string()),
        "complex pruned: {md:?}"
    );
    assert!(
        md.contains(&"simple_input".to_string()),
        "simple_input still missing: {md:?}"
    );
    assert!(!md.contains(&"mode".to_string()), "mode supplied: {md:?}");
}

#[test]
fn c3_mode_complex_prunes_simple_input() {
    let mut engine = Engine::new();
    load(&mut engine, CHOOSER);
    let mut data = HashMap::new();
    data.insert("mode".to_string(), "complex".to_string());
    let response = run(
        &engine,
        "chooser",
        data,
        Some(&["result".to_string()]),
        false,
    );
    let md = missing(&response, "result");
    assert!(
        !md.contains(&"simple_input".to_string()),
        "simple pruned: {md:?}"
    );
    assert!(md.contains(&"complex_input_a".to_string()), "{md:?}");
    assert!(md.contains(&"complex_input_b".to_string()), "{md:?}");
}

#[test]
fn c4_unless_wins_releases_default_arm_data() {
    // Default arm uses `expensive`; unless arm uses `threshold`. When unless wins,
    // on_arm_taken releases default-only paths — expensive absent from missing_data.
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data expensive: number
data threshold: number
rule main: expensive * 2
  unless now > 1970-01-01 then threshold + 1
"#,
    );
    let mut data = HashMap::new();
    data.insert("threshold".to_string(), "4".to_string());
    let response = run(&engine, "demo", data, Some(&["main".to_string()]), false);
    let md = missing(&response, "main");
    assert!(
        !md.contains(&"expensive".to_string()),
        "unless win must release default-only expensive: {md:?}"
    );
    assert!(
        !md.contains(&"threshold".to_string()),
        "threshold supplied: {md:?}"
    );
    assert!(
        md.is_empty(),
        "no unbound live inputs when unless wins with threshold: {md:?}"
    );
}

#[test]
fn c5_statically_dead_unless_arm_data_absent() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data flag: boolean
rule main: 1
  unless flag and (1 > 2) then 2
"#,
    );
    let response = run(&engine, "demo", HashMap::new(), None, false);
    let md = missing(&response, "main");
    assert!(
        !md.contains(&"flag".to_string()),
        "statically dead unless must not require flag: {md:?}"
    );
    assert!(
        !response.results["main"].vetoed,
        "statically collapsed default must evaluate: {:?}",
        response.results["main"]
    );
}

// --- D. And / remaining-live ---

#[test]
fn d1_false_and_does_not_list_right_operand() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data flag: boolean
data expensive: number
rule main: flag and expensive > 0
"#,
    );
    let mut data = HashMap::new();
    data.insert("flag".to_string(), "false".to_string());
    let response = run(&engine, "demo", data, None, false);
    let md = missing(&response, "main");
    assert!(
        !md.contains(&"expensive".to_string()),
        "false and must not need expensive: {md:?}"
    );
}

#[test]
fn d2_left_missing_still_lists_remaining_operands() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data a: number
data b: number
data c: number
rule main: a + b + c
"#,
    );
    let response = run(&engine, "demo", HashMap::new(), None, false);
    let md = missing(&response, "main");
    assert!(md.contains(&"a".to_string()), "{md:?}");
    assert!(md.contains(&"b".to_string()), "{md:?}");
    assert!(md.contains(&"c".to_string()), "{md:?}");
}

// --- E. Embeds / references ---

#[test]
fn e1_rule_embed_surfaces_inner_unbound_on_outer_missing_data() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec inner
data slot: number
rule half: slot / 2

spec outer
uses i: inner
rule r: i.half + 1
"#,
    );
    let response = run(&engine, "outer", HashMap::new(), None, false);
    let md = missing(&response, "r");
    assert!(
        md.iter()
            .any(|k| k == "i.slot" || k.ends_with(".slot") || k == "slot"),
        "outer must list inner unbound slot via embed: {md:?}"
    );
}

#[test]
fn e2_with_rule_ref_binding_omitted_from_missing_data() {
    let code = r#"
spec bag
data weight: number
data item_cost: number
rule total: weight * item_cost

spec calc
uses bag
with bag.item_cost: item_cost
data type_of_nut: text -> options "peanut" "cashew"
rule item_cost: 1
  unless type_of_nut is "cashew" then 2
rule total: bag.total
"#;
    let mut engine = Engine::new();
    engine
        .load([(
            lemma::SourceType::Path(Arc::new(std::path::PathBuf::from("with_bind.lemma"))),
            code.to_string(),
        )])
        .expect("load");
    let response = run(
        &engine,
        "calc",
        HashMap::new(),
        Some(&["total".to_string()]),
        false,
    );
    let md = missing(&response, "total");
    assert!(
        !md.contains(&"bag.item_cost".to_string()),
        "with-bound rule ref must not appear: {md:?}"
    );
}

#[test]
fn e3_nested_input_key_matches_show_data_key() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec inner
data slot: number
rule half: slot / 2

spec outer
uses i: inner
rule r: i.half
"#,
    );
    let response = run(&engine, "outer", HashMap::new(), None, false);
    let md = missing(&response, "r");
    assert!(!md.is_empty(), "expected nested missing key: {md:?}");
    for key in md {
        show_has(&engine, "outer", key);
    }
}

// --- F. Explain parity ---

#[test]
fn f1_explain_true_false_same_missing_data_and_outcomes() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data a: number
data b: number
rule main: a + b
"#,
    );
    let mut data = HashMap::new();
    data.insert("a".to_string(), "1".to_string());
    let plain = run(&engine, "demo", data.clone(), None, false);
    let explained = run(&engine, "demo", data, None, true);
    assert_eq!(
        missing(&plain, "main"),
        missing(&explained, "main"),
        "missing_data must match"
    );
    let p = rule(&plain, "main");
    let e = rule(&explained, "main");
    assert_eq!(p.vetoed, e.vetoed);
    assert_eq!(p.display, e.display);
    assert_eq!(p.veto_reason, e.veto_reason);
}

#[test]
fn f2_fully_supplied_with_explain_missing_data_empty() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data a: number
rule main: a
"#,
    );
    let mut data = HashMap::new();
    data.insert("a".to_string(), "9".to_string());
    let response = run(&engine, "demo", data, None, true);
    assert!(
        missing(&response, "main").is_empty(),
        "{:?}",
        missing(&response, "main")
    );
}

// --- G. Structure / show keys / order ---

#[test]
fn g2_every_missing_data_key_exists_in_show() {
    let mut engine = Engine::new();
    load(
        &mut engine,
        r#"
spec demo
data a: number
data b: number
rule main: a + b
"#,
    );
    let response = run(&engine, "demo", HashMap::new(), None, false);
    let md = missing(&response, "main");
    assert_eq!(md, &["a", "b"]);
    for key in md {
        show_has(&engine, "demo", key);
    }
}

#[test]
fn g3_missing_data_order_follows_plan_data_declaration() {
    let mut engine = Engine::new();
    load(&mut engine, CHOOSER);
    let response = run(
        &engine,
        "chooser",
        HashMap::new(),
        Some(&["result".to_string()]),
        false,
    );
    assert_eq!(
        missing(&response, "result"),
        &["mode", "simple_input", "complex_input_a", "complex_input_b"]
    );
}
