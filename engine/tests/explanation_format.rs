use lemma::{Engine, SourceType};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

fn explanation_json(response: &lemma::Response, rule: &str) -> Value {
    serde_json::to_value(response).unwrap()["results"][rule]["explanation"].clone()
}

#[test]
fn explanation_unless_default_matched() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec t
data x: text -> option "a" -> option "b"
rule out: 1 unless x is "b" then 2
"#
            .to_string(),
        )])
        .unwrap();
    let mut data = HashMap::new();
    data.insert("x".into(), "a".into());
    let response = engine
        .run(None, "t", None, data, Some(&["out".to_string()]), true)
        .unwrap();
    let explanation = explanation_json(&response, "out");

    assert_eq!(explanation["type"], "rule");
    assert_eq!(explanation["name"], "out");
    assert_eq!(explanation["result"], "1");
    assert_eq!(explanation["body"], "1");
    let causes = explanation["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 1);
    assert_eq!(causes[0]["condition"], "x is not b");
    assert_eq!(causes[0]["value"], "true");
    let cause_children = causes[0]["children"].as_array().unwrap();
    assert_eq!(cause_children[0]["name"], "x");
    assert_eq!(cause_children[0]["display"], "a");
}

#[test]
fn explanation_compose_with_data_operands() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec t
data money: measure -> unit eur 1 -> decimals 2
data price: 100 eur
data quantity: number
data q: 3
rule total: price * q
"#
            .to_string(),
        )])
        .unwrap();
    let response = engine
        .run(
            None,
            "t",
            None,
            HashMap::new(),
            Some(&["total".to_string()]),
            true,
        )
        .unwrap();
    let explanation = explanation_json(&response, "total");

    assert_eq!(explanation["body"], "price * q");
    let children = explanation["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["type"], "data");
    assert_eq!(children[0]["name"], "price");
    assert_eq!(children[0]["display"], "100.00 eur");
    assert_eq!(children[1]["name"], "q");
    assert_eq!(children[1]["display"], "3");
}

#[test]
fn explanation_rule_addition_expands_both_rules() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec t
data n: number
data x: 5
rule base: x * 2
rule a: base + 1
rule b: a + base
"#
            .to_string(),
        )])
        .unwrap();
    let response = engine
        .run(
            None,
            "t",
            None,
            HashMap::new(),
            Some(&["b".to_string()]),
            true,
        )
        .unwrap();
    let explanation = explanation_json(&response, "b");

    assert_eq!(explanation["body"], "a + base");
    let children = explanation["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["type"], "rule");
    assert_eq!(children[0]["name"], "a");
    assert_eq!(children[1]["type"], "rule");
    assert_eq!(children[1]["name"], "base");
}

#[test]
fn explanation_veto_missing_data() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec t
data n: number
rule out: n * 2
"#
            .to_string(),
        )])
        .unwrap();
    let response = engine
        .run(
            None,
            "t",
            None,
            HashMap::new(),
            Some(&["out".to_string()]),
            true,
        )
        .unwrap();
    let explanation = explanation_json(&response, "out");

    assert_eq!(explanation["result"], "Missing data: n");
    // Walk-faithful: missing data leaf is data; veto text is on result (and display).
    let children = explanation["children"].as_array().unwrap();
    assert_eq!(children[0]["type"], "data");
    assert_eq!(children[0]["name"], "n");
    assert!(children[0]["display"]
        .as_str()
        .unwrap()
        .contains("Missing data: n"));
}

#[test]
fn explanation_always_built_by_engine() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec t
data n: number
data x: 5
rule out: x + 1
"#
            .to_string(),
        )])
        .unwrap();
    let response = engine
        .run(
            None,
            "t",
            None,
            HashMap::new(),
            Some(&["out".to_string()]),
            true,
        )
        .unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    assert!(json["results"]["out"]["explanation"].is_object());
}

#[test]
fn explanation_json_compact_for_net_salary() {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nl/tax/net_salary.lemma"),
    )
    .expect("read net_salary fixture");
    let mut engine = Engine::new();
    engine
        .load([(SourceType::Volatile, &source.to_string())])
        .unwrap();
    let mut data = HashMap::new();
    data.insert("gross_salary".into(), "5000 eur".into());
    data.insert("pay_period".into(), "month".into());
    let response = engine
        .run(
            None,
            "net_salary",
            None,
            data,
            Some(&["net_salary".to_string()]),
            true,
        )
        .unwrap();
    let explanation = response
        .get("net_salary")
        .expect("net_salary evaluated")
        .explanation
        .as_ref()
        .expect("explanation");
    let json_string = serde_json::to_string_pretty(explanation).unwrap();
    let line_count = json_string.lines().count();
    // Rule references embed their full explanation tree wherever they appear
    // (no dedup/collapse heuristics), so the JSON grows with the dependency
    // tree. This bound guards against accidental unbounded regressions.
    assert!(
        line_count < 6000,
        "Single-rule explanation JSON should stay bounded with full embedded rule subtrees, got {line_count} lines"
    );
}

#[test]
fn explanation_logical_and_in_body() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec t
data contract_start: 2020-01-01
data contract_end: 2030-01-01
data current_date: 2025-01-01
rule active: current_date >= contract_start and current_date <= contract_end
"#
            .to_string(),
        )])
        .unwrap();
    let response = engine
        .run(
            None,
            "t",
            None,
            HashMap::new(),
            Some(&["active".to_string()]),
            true,
        )
        .unwrap();
    let explanation = explanation_json(&response, "active");

    assert!(
        explanation["body"].as_str().unwrap().contains(" and "),
        "expected and in body, got: {}",
        explanation["body"]
    );
    assert_eq!(explanation["result"], "true");
}

#[test]
fn explanation_unit_conversion() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec t
data weight: measure -> unit kg 1 -> unit gram 0.001
data w: 2 kg
rule in_grams: w as gram
"#
            .to_string(),
        )])
        .unwrap();
    let response = engine
        .run(
            None,
            "t",
            None,
            HashMap::new(),
            Some(&["in_grams".to_string()]),
            true,
        )
        .unwrap();
    let explanation = explanation_json(&response, "in_grams");

    let children = explanation["children"].as_array().unwrap();
    assert_eq!(children[0]["type"], "conversion");
    let steps = children[0]["steps"].as_array().unwrap();
    assert!(steps.iter().any(|s| s["role"] == "outcome"));
    assert!(steps.iter().any(|s| s["role"] == "source"));
    assert!(steps
        .iter()
        .any(|s| s["role"] == "rule" && s["text"].as_str().unwrap().contains("1000")));
}

#[test]
fn explain_parameter_gates_explanation_build() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec t
data x: text -> option "a" -> option "b"
rule out: 1 unless x is "b" then 2
"#
            .to_string(),
        )])
        .unwrap();
    let mut data = HashMap::new();
    data.insert("x".into(), "a".into());
    let without = engine
        .run(None, "t", None, data.clone(), None, false)
        .unwrap();
    assert!(without.results["out"].explanation.is_none());
    let with = engine
        .run(None, "t", None, data, Some(&["out".to_string()]), true)
        .unwrap();
    assert!(with.results["out"].explanation.is_some());
}
