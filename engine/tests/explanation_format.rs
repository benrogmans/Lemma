use lemma::Engine;
use lemma::SourceType;
use serde_json::Value;
use std::collections::HashMap;

#[test]
fn explanation_branches_default_matched() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data x: text -> option "a" -> option "b"
rule out: 1 unless x is "b" then 2
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let mut data = HashMap::new();
    data.insert("x".into(), "a".into());
    let response = engine.run(None, "t", None, data, true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    let explanation = &json["results"]["out"]["explanation"];

    assert_eq!(explanation["rule"], "out");
    assert_eq!(explanation["result"], "1");

    let tree = &explanation["tree"];
    assert_eq!(tree["type"], "branches");
    assert_eq!(tree["matched"]["tree"]["type"], "value");
    assert_eq!(tree["matched"]["tree"]["display"], "1");
    assert!(tree["matched"]["condition"].is_null());
    let condition = tree["non_matched"][0]["condition"].as_str().unwrap();
    assert!(
        condition.contains('b'),
        "expected unless condition to reference b, got: {condition}"
    );
}

#[test]
fn explanation_computation_with_operands() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data money: quantity -> unit eur 1 -> decimals 2
data price: 100 eur
data qty: number
data q: 3
rule total: price * q
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let response = engine.run(None, "t", None, HashMap::new(), true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    let tree = &json["results"]["total"]["explanation"]["tree"];

    assert_eq!(tree["type"], "computation");
    assert_eq!(tree["result"], "300.00 eur");
    assert_eq!(tree["expression"], "100.00 eur * 3");
    // Operands are data references
    assert_eq!(tree["operands"][0]["type"], "value");
    assert_eq!(tree["operands"][0]["data"], "price");
    assert_eq!(tree["operands"][0]["display"], "100.00 eur");
    assert_eq!(tree["operands"][1]["data"], "q");
    assert_eq!(tree["operands"][1]["display"], "3");
}

#[test]
fn explanation_rule_reference_deduplicated() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data n: number
data x: 5
rule base: x * 2
rule a: base + 1
rule b: a + base
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let response = engine.run(None, "t", None, HashMap::new(), true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();

    // In rule 'b', 'base' is referenced. It was already expanded in 'a'.
    // But each rule's explanation is self-contained — deduplication is within a single rule's tree.
    let b_tree = &json["results"]["b"]["explanation"]["tree"];
    assert_eq!(b_tree["type"], "computation");

    // 'a' reference should have a tree (first occurrence in b's explanation)
    let a_operand = &b_tree["operands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|op| op["rule"] == "a")
        .unwrap();
    assert!(a_operand["tree"].is_object());

    // Within a's expansion, 'base' is expanded
    // Then 'base' direct operand on b should NOT have tree (already seen within a's expansion)
    let base_operand = &b_tree["operands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|op| op["rule"] == "base")
        .unwrap();
    assert!(base_operand.get("tree").is_none());
}

#[test]
fn explanation_veto_missing_data() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data n: number
rule out: n * 2
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let response = engine.run(None, "t", None, HashMap::new(), true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    let explanation = &json["results"]["out"]["explanation"];

    assert_eq!(explanation["result"], "Missing data: n");
    let tree = &explanation["tree"];
    assert_eq!(tree["type"], "veto");
    assert!(
        tree["message"].as_str().unwrap().contains("n"),
        "expected veto message to contain 'n', got: {:?}",
        tree["message"]
    );
}

#[test]
fn no_explanation_when_not_requested() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data n: number
data x: 5
rule out: x + 1
"#,
            SourceType::Volatile,
        )
        .unwrap();
    // explain = false — no consumer-facing explanation generated
    let response = engine.run(None, "t", None, HashMap::new(), false).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    // "explanation" key must be absent (Option::None, skipped by serde)
    assert!(!json["results"]["out"]
        .as_object()
        .unwrap()
        .contains_key("explanation"));
}

#[test]
fn explanation_json_compact_for_net_salary() {
    let source =
        std::fs::read_to_string("../documentation/examples/06_dutch_net_salary.lemma").unwrap();
    let mut engine = Engine::new();
    engine.load(&source, SourceType::Volatile).unwrap();
    let mut data = HashMap::new();
    data.insert("gross_salary".into(), "5000 eur".into());
    data.insert("pay_period".into(), "month".into());
    let response = engine.run(None, "net_salary", None, data, true).unwrap();
    let explanation = response
        .results
        .values()
        .find_map(|r| r.trace.as_ref())
        .expect("at least one rule should have explanation");
    let json_string = serde_json::to_string_pretty(explanation).unwrap();
    let line_count = json_string.lines().count();
    assert!(
        line_count < 500,
        "Single-rule explanation JSON should be compact, got {} lines",
        line_count
    );
}

#[test]
fn explanation_logical_and_shows_operation_with_operands() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data contract_start: 2020-01-01
data contract_end: 2030-01-01
data current_date: 2025-01-01
rule active: current_date >= contract_start and current_date <= contract_end
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let response = engine.run(None, "t", None, HashMap::new(), true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    let tree = &json["results"]["active"]["explanation"]["tree"];

    assert_eq!(tree["type"], "computation");
    assert!(
        tree["expression"].as_str().unwrap().contains(" and "),
        "expected and operation in expression, got: {}",
        tree["expression"]
    );
    assert_eq!(tree["result"], "true");
    let operands = tree["operands"].as_array().unwrap();
    assert_eq!(
        operands.len(),
        2,
        "expected both and operands in explanation, got: {operands:?}"
    );
}

#[test]
fn explanation_sqrt_shows_computation_with_operand() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
rule root: sqrt 9
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let response = engine.run(None, "t", None, HashMap::new(), true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    let tree = &json["results"]["root"]["explanation"]["tree"];

    assert_eq!(tree["type"], "computation");
    assert_eq!(tree["expression"], "sqrt 9");
    assert_eq!(tree["result"], "3");
    let operands = tree["operands"].as_array().unwrap();
    assert_eq!(operands.len(), 1);
    assert_eq!(operands[0]["type"], "value");
    assert_eq!(operands[0]["display"], "9");
}

#[test]
fn explanation_is_veto_positive_phrasing_when_operand_vetoed() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data price: -5
rule validated_price: price unless price < 0 then veto "negative"
rule flag: validated_price is veto
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let response = engine.run(None, "t", None, HashMap::new(), true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    let tree = &json["results"]["flag"]["explanation"]["tree"];

    assert_eq!(tree["type"], "computation");
    assert_eq!(tree["result"], "true");
    assert!(
        tree["expression"].as_str().unwrap().contains("is veto"),
        "expected positive phrasing for vetoed operand, got: {}",
        tree["expression"]
    );
}

#[test]
fn explanation_is_veto_positive_phrasing_when_operand_not_vetoed() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data price: 10
rule validated_price: price unless price < 0 then veto "negative"
rule flag: validated_price is veto
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let response = engine.run(None, "t", None, HashMap::new(), true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    let tree = &json["results"]["flag"]["explanation"]["tree"];

    assert_eq!(tree["type"], "computation");
    assert_eq!(tree["result"], "true");
    assert!(
        tree["expression"].as_str().unwrap().contains("is not veto"),
        "expected positive phrasing for non-vetoed operand, got: {}",
        tree["expression"]
    );
}

#[test]
fn explanation_unit_conversion() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec t
data weight: quantity -> unit kg 1 -> unit gram 0.001
data w: 2 kg
rule in_grams: w as gram
"#,
            SourceType::Volatile,
        )
        .unwrap();
    let response = engine.run(None, "t", None, HashMap::new(), true).unwrap();
    let json: Value = serde_json::to_value(&response).unwrap();
    let tree = &json["results"]["in_grams"]["explanation"]["tree"];

    assert_eq!(tree["type"], "conversion");
    let steps = tree["steps"].as_array().unwrap();
    assert!(steps.iter().any(|s| s["role"] == "outcome"));
    assert!(steps.iter().any(|s| s["role"] == "source"));
    // Rule step present because kg != gram
    assert!(steps
        .iter()
        .any(|s| s["role"] == "rule" && s["text"].as_str().unwrap().contains("1000")));
}
