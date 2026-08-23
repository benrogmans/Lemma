//! Integration contracts: explanations narrate everything normalization folds away.
//! Exact `assert_eq!` on typed Explanation/Cause fields and JSON node paths — never `.contains()`.

use lemma::{DateTimeValue, Engine, Explanation};
use std::collections::HashMap;

fn load(code: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .expect("spec must load");
    engine
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

fn out_explanation(response: &lemma::Response) -> &Explanation {
    response
        .results
        .get("out")
        .expect("out in response")
        .explanation
        .as_ref()
        .expect("out explanation")
}

fn cause_pairs(explanation: &Explanation) -> Vec<(&str, &str)> {
    explanation
        .causes
        .iter()
        .map(|c| (c.condition.as_str(), c.value.as_str()))
        .collect()
}

fn explanation_json(explanation: &Explanation) -> serde_json::Value {
    serde_json::to_value(explanation).expect("serialize explanation")
}

fn compose_expressions(value: &serde_json::Value) -> Vec<&str> {
    let mut found = Vec::new();
    fn walk<'a>(value: &'a serde_json::Value, found: &mut Vec<&'a str>) {
        match value {
            serde_json::Value::Object(obj) => {
                if obj.get("type").and_then(|t| t.as_str()) == Some("compose") {
                    if let Some(expr) = obj.get("expression").and_then(|e| e.as_str()) {
                        found.push(expr);
                    }
                }
                for child in obj.values() {
                    walk(child, found);
                }
            }
            serde_json::Value::Array(arr) => {
                for child in arr {
                    walk(child, found);
                }
            }
            _ => {}
        }
    }
    walk(value, &mut found);
    found
}

fn rule_names(value: &serde_json::Value) -> Vec<&str> {
    let mut found = Vec::new();
    fn walk<'a>(value: &'a serde_json::Value, found: &mut Vec<&'a str>) {
        match value {
            serde_json::Value::Object(obj) => {
                if obj.get("type").and_then(|t| t.as_str()) == Some("rule") {
                    if let Some(name) = obj.get("name").and_then(|r| r.as_str()) {
                        found.push(name);
                    }
                }
                for child in obj.values() {
                    walk(child, found);
                }
            }
            serde_json::Value::Array(arr) => {
                for child in arr {
                    walk(child, found);
                }
            }
            _ => {}
        }
    }
    walk(value, &mut found);
    found
}

fn conversion_expressions(value: &serde_json::Value) -> Vec<&str> {
    let mut found = Vec::new();
    fn walk<'a>(value: &'a serde_json::Value, found: &mut Vec<&'a str>) {
        match value {
            serde_json::Value::Object(obj) => {
                if obj.get("type").and_then(|t| t.as_str()) == Some("conversion") {
                    if let Some(expr) = obj.get("expression").and_then(|e| e.as_str()) {
                        found.push(expr);
                    }
                }
                for child in obj.values() {
                    walk(child, found);
                }
            }
            serde_json::Value::Array(arr) => {
                for child in arr {
                    walk(child, found);
                }
            }
            _ => {}
        }
    }
    walk(value, &mut found);
    found
}

// ── Suite 1: piecewise collapse must narrate ─────────────────────────────

#[test]
fn static_false_comparison_unless_explains_flipped_fact() {
    let engine = load(
        r#"
spec flipped_cmp
rule out: true
  unless 5 < 3 then false
"#,
    );
    let response = run(
        &engine,
        "flipped_cmp",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    let out = response.results.get("out").expect("out");
    assert_eq!(out.display(), Some("true"));
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "true");
    assert_eq!(cause_pairs(explanation), vec![("5 >= 3", "true")]);
}

#[test]
fn static_false_literal_unless_explains_falsified_arm() {
    let engine = load(
        r#"
spec falsified_literal
rule out: 1 unless false then 2
"#,
    );
    let response = run(
        &engine,
        "falsified_literal",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("1")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "1");
    assert_eq!(cause_pairs(explanation), vec![("false", "false")]);
}

#[test]
fn static_true_unless_explains_held_winning_condition() {
    let engine = load(
        r#"
spec held_true
rule out: 1 unless true then 2
"#,
    );
    let response = run(
        &engine,
        "held_true",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("2")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "2");
    assert_eq!(cause_pairs(explanation), vec![("true", "true")]);
}

#[test]
fn static_true_comparison_unless_explains_winning_comparison() {
    let engine = load(
        r#"
spec held_cmp
rule out: 1 unless 5 > 3 then 2
"#,
    );
    let response = run(
        &engine,
        "held_cmp",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("2")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "2");
    assert_eq!(cause_pairs(explanation), vec![("5 > 3", "true")]);
}

#[test]
fn static_true_winner_omits_shadowed_earlier_arms() {
    let engine = load(
        r#"
spec shadowed
data x: boolean
rule out: 1 unless x then 2 unless true then 3
"#,
    );
    let response = run(
        &engine,
        "shadowed",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("3")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "3");
    assert_eq!(cause_pairs(explanation), vec![("true", "true")]);
}

#[test]
fn partial_dead_arm_among_live_piecewise_narrates_dead() {
    let engine = load(
        r#"
spec partial_dead
data x: boolean
rule out: 1 unless false then 2 unless x then 3
"#,
    );
    let data = HashMap::from([("x".into(), "true".into())]);
    let response = run(
        &engine,
        "partial_dead",
        data,
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("3")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "3");
    assert_eq!(
        cause_pairs(explanation),
        vec![("false", "false"), ("x is true", "true")]
    );
}

#[test]
fn and_false_conjunct_in_unless_keeps_flag_in_explanation() {
    let engine = load(
        r#"
spec and_false_flag
data flag: boolean
rule out: 1 unless flag and false then 2
"#,
    );
    let response = run(
        &engine,
        "and_false_flag",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("1")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "1");
    assert_eq!(explanation.causes.len(), 1);
    assert_eq!(explanation.causes[0].condition, "flag and false");
    assert_eq!(explanation.causes[0].value, "false");
    let json = explanation_json(explanation);
    let children = json["causes"][0]["children"]
        .as_array()
        .expect("cause children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["type"], "data_unused");
    assert_eq!(children[0]["name"], "flag");
    assert!(
        children[0].get("display").is_none(),
        "unused flag has no display field, got {}",
        children[0]
    );
    let formatted = lemma::format_explanation(explanation);
    assert!(
        formatted.contains("flag"),
        "formatter must render unused path, got {formatted}"
    );
}

#[test]
fn bound_and_false_in_unless_fills_cause_displays_and_formats() {
    let engine = load(
        r#"
spec bound_and
data a: boolean
data b: boolean
rule out: "ok"
  unless a and b then "both"
"#,
    );
    let mut data = HashMap::new();
    data.insert("a".into(), "false".into());
    data.insert("b".into(), "true".into());
    let response = run(&engine, "bound_and", data, Some(&["out".to_string()]), true);
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("ok")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.causes.len(), 1);
    assert_eq!(explanation.causes[0].condition, "a and b");
    assert_eq!(explanation.causes[0].value, "false");
    let json = explanation_json(explanation);
    let children = json["causes"][0]["children"]
        .as_array()
        .expect("cause children");
    assert_eq!(children.len(), 2);
    let mut by_data = std::collections::HashMap::new();
    for child in children {
        assert_eq!(child["type"], "data");
        by_data.insert(
            child["name"].as_str().expect("name").to_string(),
            child["display"].as_str().expect("display").to_string(),
        );
    }
    assert_eq!(by_data.get("a").map(String::as_str), Some("false"));
    assert_eq!(by_data.get("b").map(String::as_str), Some("true"));
    let formatted = lemma::format_explanation(explanation);
    assert!(
        formatted.contains("a: false") && formatted.contains("b: true"),
        "formatter must render bound And cause children, got {formatted}"
    );
}

#[test]
fn and_short_circuit_unused_sibling_stays_unused() {
    let engine = load(
        r#"
spec short_and
data a: boolean
data b: boolean
rule out: "ok"
  unless a and b then "both"
"#,
    );
    let mut data = HashMap::new();
    data.insert("a".into(), "false".into());
    let response = run(&engine, "short_and", data, Some(&["out".to_string()]), true);
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("ok")
    );
    let explanation = out_explanation(&response);
    let json = explanation_json(explanation);
    let children = json["causes"][0]["children"]
        .as_array()
        .expect("cause children");
    assert_eq!(children.len(), 2);
    let mut types = std::collections::HashMap::new();
    for child in children {
        types.insert(
            child["name"].as_str().expect("name").to_string(),
            child["type"].as_str().expect("type").to_string(),
        );
    }
    assert_eq!(types.get("a").map(String::as_str), Some("data"));
    assert_eq!(types.get("b").map(String::as_str), Some("data_unused"));
    let formatted = lemma::format_explanation(explanation);
    assert!(
        formatted.contains("a: false") && formatted.contains("b"),
        "formatter must render bound and unused And children, got {formatted}"
    );
}

#[test]
fn mixed_static_false_then_static_true_only_held_cause() {
    let engine = load(
        r#"
spec mixed_static
rule out: 0 unless 1 < 0 then 1 unless true then 2
"#,
    );
    let response = run(
        &engine,
        "mixed_static",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("2")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "2");
    assert_eq!(cause_pairs(explanation), vec![("true", "true")]);
}

// ── Suite 2: algebra provenance ──────────────────────────────────────────

#[test]
fn distinct_sqrt_product_preserves_both_sqrt_preimages() {
    let engine = load(
        r#"
spec sqrt_product
rule out: (sqrt 4) * (sqrt 9)
"#,
    );
    let response = run(
        &engine,
        "sqrt_product",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("6")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "sqrt(4) * sqrt(9)");
    let json = explanation_json(explanation);
    let mut sqrts: Vec<&str> = compose_expressions(&json)
        .into_iter()
        .filter(|e| *e == "sqrt(4)" || *e == "sqrt(9)")
        .collect();
    sqrts.sort_unstable();
    sqrts.dedup();
    assert_eq!(sqrts, vec!["sqrt(4)", "sqrt(9)"]);
}

#[test]
fn sqrt_of_distinct_literals_fold_still_explains_sqrts() {
    let engine = load(
        r#"
spec sqrt_rules
rule a: sqrt 4
rule b: sqrt 9
rule out: a * b
"#,
    );
    let response = run(
        &engine,
        "sqrt_rules",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("6")
    );
    let explanation = out_explanation(&response);
    let json = explanation_json(explanation);
    let mut names = rule_names(&json);
    names.sort_unstable();
    assert_eq!(names, vec!["a", "b", "out"]);
    let mut bodies: Vec<&str> = Vec::new();
    fn walk_rule_bodies<'a>(value: &'a serde_json::Value, bodies: &mut Vec<&'a str>) {
        match value {
            serde_json::Value::Object(obj) => {
                if obj.get("type").and_then(|t| t.as_str()) == Some("rule") {
                    if let Some(body) = obj.get("body").and_then(|b| b.as_str()) {
                        bodies.push(body);
                    }
                }
                for child in obj.values() {
                    walk_rule_bodies(child, bodies);
                }
            }
            serde_json::Value::Array(arr) => {
                for child in arr {
                    walk_rule_bodies(child, bodies);
                }
            }
            _ => {}
        }
    }
    walk_rule_bodies(&json, &mut bodies);
    let mut sqrts: Vec<&str> = bodies
        .into_iter()
        .filter(|b| *b == "sqrt(4)" || *b == "sqrt(9)")
        .collect();
    sqrts.sort_unstable();
    sqrts.dedup();
    assert_eq!(sqrts, vec!["sqrt(4)", "sqrt(9)"]);
}

#[test]
fn named_compound_measure_literal_explains_named_unit() {
    let engine = load(
        r#"
spec named_rate
uses lemma units
data money: measure
  -> unit eur: 1
data rate: measure
  -> unit eur_per_hour: eur/hour
rule out: 50 eur_per_hour
"#,
    );
    let response = run(
        &engine,
        "named_rate",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("50 eur_per_hour")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "50 eur_per_hour");
}

#[test]
fn nested_identity_elim_keeps_inner_fold_in_explanation() {
    let engine = load(
        r#"
spec nested_identity
rule out: (exp(log(5))) + 0
"#,
    );
    let response = run(
        &engine,
        "nested_identity",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("5")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "exp(log(5))");
}

// ── Suite 3: conversion and order ────────────────────────────────────────

#[test]
fn number_as_number_conversion_appears_in_explanation() {
    let engine = load(
        r#"
spec as_number
rule out: 100 as number
"#,
    );
    let response = run(
        &engine,
        "as_number",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("100")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "100 as number");
    let json = explanation_json(explanation);
    assert_eq!(conversion_expressions(&json), vec!["100 as number"]);
}

#[test]
fn measure_as_unit_conversion_narrated_when_folded() {
    let engine = load(
        r#"
spec measure_as
uses lemma units
data mass: measure
  -> unit kilogram: 1
  -> unit gram: 0.001
  -> suggest 2 kilogram
rule out: mass as gram
"#,
    );
    let data = HashMap::from([("mass".into(), "2 kilogram".into())]);
    let response = run(
        &engine,
        "measure_as",
        data,
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("2000 gram")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "mass as gram");
    let json = explanation_json(explanation);
    assert_eq!(conversion_expressions(&json), vec!["mass as gram"]);
}

#[test]
fn sum_reorder_still_explains_source_operand_identity() {
    let engine = load(
        r#"
spec sum_order
rule out: 2 + 1
"#,
    );
    let response = run(
        &engine,
        "sum_order",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("3")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "1 + 2");
    let json = explanation_json(explanation);
    assert_eq!(
        json["body"], "1 + 2",
        "sum fold must narrate ordered sum pre-image"
    );
}

// ── Suite 4: cross-cutting ───────────────────────────────────────────────

#[test]
fn static_unless_explain_false_value_parity() {
    let engine = load(
        r#"
spec parity_static
rule out: true unless 5 < 3 then false
"#,
    );
    let rules = ["out".to_string()];
    let without = run(
        &engine,
        "parity_static",
        HashMap::new(),
        Some(&rules),
        false,
    );
    let with = run(&engine, "parity_static", HashMap::new(), Some(&rules), true);
    let left = without.results.get("out").expect("out");
    let right = with.results.get("out").expect("out");
    assert_eq!(left.vetoed, right.vetoed);
    assert_eq!(left.display(), right.display());
    assert!(left.explanation.is_none());
    assert!(right.explanation.is_some());
}

#[test]
fn chained_static_unless_on_rule_ref() {
    let engine = load(
        r#"
spec chained_ref
rule base: 1 unless false then 9
rule out: base
"#,
    );
    let response = run(
        &engine,
        "chained_ref",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("1")
    );
    let explanation = out_explanation(&response);
    let json = explanation_json(explanation);
    assert_eq!(rule_names(&json), vec!["out", "base"]);
    let base = json["children"]
        .as_array()
        .expect("children")
        .iter()
        .find(|n| n.get("name").and_then(|r| r.as_str()) == Some("base"))
        .expect("base embed");
    assert_eq!(base["causes"][0]["condition"], "false");
    assert_eq!(base["causes"][0]["value"], "false");
}

#[test]
fn runtime_falsified_unless_still_flips() {
    let engine = load(
        r#"
spec runtime_flip
data n: 5
rule out: true unless n < 3 then false
"#,
    );
    let response = run(
        &engine,
        "runtime_flip",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("true")
    );
    let explanation = out_explanation(&response);
    assert_eq!(cause_pairs(explanation), vec![("n >= 3", "true")]);
}

#[test]
fn last_match_wins_static_true_over_earlier_true() {
    let engine = load(
        r#"
spec two_true
rule out: 0 unless true then 1 unless true then 2
"#,
    );
    let response = run(
        &engine,
        "two_true",
        HashMap::new(),
        Some(&["out".to_string()]),
        true,
    );
    assert_eq!(
        response.results.get("out").expect("out").display(),
        Some("2")
    );
    let explanation = out_explanation(&response);
    assert_eq!(explanation.body, "2");
    assert_eq!(cause_pairs(explanation), vec![("true", "true")]);
}
