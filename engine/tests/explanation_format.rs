use lemma::{Engine, SourceType};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

fn explanation_json(response: &lemma::Response, rule: &str) -> Value {
    serde_json::to_value(response).unwrap()["results"][rule]["explanation"].clone()
}

fn compose_nodes_matching<'a>(value: &'a Value, needle: &str) -> Vec<&'a Value> {
    let mut found = Vec::new();
    fn walk<'a>(value: &'a Value, needle: &str, found: &mut Vec<&'a Value>) {
        match value {
            Value::Object(obj) => {
                if obj.get("type").and_then(|t| t.as_str()) == Some("compose")
                    && obj
                        .get("expression")
                        .and_then(|e| e.as_str())
                        .is_some_and(|expr| expr.contains(needle))
                {
                    found.push(value);
                }
                for child in obj.values() {
                    walk(child, needle, found);
                }
            }
            Value::Array(arr) => {
                for child in arr {
                    walk(child, needle, found);
                }
            }
            _ => {}
        }
    }
    walk(value, needle, &mut found);
    found
}

fn assert_rule_binary_right_veto_children(explanation: &Value, body_needle: &str) {
    assert!(
        explanation["body"]
            .as_str()
            .is_some_and(|body| body.contains(body_needle)),
        "rule body must retain binary expression, got: {explanation}"
    );
    let children = explanation["children"]
        .as_array()
        .expect("rule explanation must have children");
    assert_eq!(
        children.len(),
        2,
        "binary right veto must expose both operands under rule, got: {explanation}"
    );
    assert_eq!(
        children[0]["type"], "compose",
        "left settled operand must be compose child: {explanation}"
    );
    assert_eq!(
        children[1]["type"], "rule",
        "right veto embed must remain second operand: {explanation}"
    );
    assert_eq!(children[1]["name"], "base");
}

fn run_settled_left_right_veto_explanation(
    engine: &mut Engine,
    spec: &str,
    rule_body: &str,
) -> Value {
    engine
        .load([(
            SourceType::Volatile,
            format!(
                r#"
spec demo
data age: number
rule base: veto "no rate"
  unless age < 70 then 10
rule main: {rule_body}
"#
            ),
        )])
        .expect("load");
    let mut data = HashMap::new();
    data.insert("age".to_string(), "80".to_string());
    let response = engine
        .run(None, spec, None, data, Some(&["main".to_string()]), true)
        .expect("evaluation must succeed");
    explanation_json(&response, "main")
}

#[test]
fn explanation_comparison_right_veto_compose() {
    let mut engine = Engine::new();
    let explanation = run_settled_left_right_veto_explanation(&mut engine, "demo", "100 > base");
    assert_rule_binary_right_veto_children(&explanation, ">");
    assert_eq!(explanation["body"], "100 > base");
    assert_eq!(explanation["children"][0]["expression"], "100");
}

#[test]
fn explanation_range_literal_right_veto_compose() {
    let mut engine = Engine::new();
    let explanation = run_settled_left_right_veto_explanation(&mut engine, "demo", "0...base");
    assert_rule_binary_right_veto_children(&explanation, "base");
    assert!(
        explanation["body"]
            .as_str()
            .is_some_and(|body| body.contains("0") && body.contains("base")),
        "range literal body must name both endpoints: {}",
        explanation["body"]
    );
    assert_eq!(explanation["children"][0]["expression"], "0");
}

#[test]
fn explanation_range_containment_right_veto_compose() {
    let mut engine = Engine::new();
    let explanation =
        run_settled_left_right_veto_explanation(&mut engine, "demo", "50 in 0...base");
    assert!(
        explanation["body"]
            .as_str()
            .is_some_and(|body| body.contains("50") && body.contains("in")),
        "range containment body must name value and range: {}",
        explanation["body"]
    );
    let children = explanation["children"]
        .as_array()
        .expect("rule explanation must have children");
    assert_eq!(
        children.len(),
        2,
        "range containment right veto must expose value and range operands: {explanation}"
    );
    assert_eq!(children[0]["type"], "compose");
    assert_eq!(children[0]["expression"], "50");
    assert_eq!(children[1]["type"], "compose");
    let range = compose_nodes_matching(&children[1], "to");
    assert!(
        !range.is_empty(),
        "range operand must remain a compose subtree: {explanation}"
    );
    fn has_rule_named_base(value: &Value) -> bool {
        match value {
            Value::Object(obj) => {
                if obj.get("type").and_then(|t| t.as_str()) == Some("rule")
                    && obj.get("name").and_then(|n| n.as_str()) == Some("base")
                {
                    return true;
                }
                obj.values().any(has_rule_named_base)
            }
            Value::Array(arr) => arr.iter().any(has_rule_named_base),
            _ => false,
        }
    }
    assert!(
        has_rule_named_base(&children[1]),
        "range compose must still embed vetoing base rule: {explanation}"
    );
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
data money: measure -> unit eur: 1 -> decimals 2
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
data weight: measure -> unit kg: 1 -> unit gram: 0.001
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
