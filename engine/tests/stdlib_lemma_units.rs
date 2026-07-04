use lemma::DateTimeValue;
use lemma::{Engine, SourceType};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn path_source(file: &str) -> SourceType {
    SourceType::Path(Arc::new(PathBuf::from(file)))
}

fn eval_rule(code: &str, spec_name: &str, rule_name: &str) -> String {
    let mut engine = Engine::new();
    engine
        .load(code, path_source("stdlib_lemma_units.lemma"))
        .expect("load");
    let now = DateTimeValue::now();
    let response = engine
        .run(None, spec_name, Some(&now), HashMap::new(), false, None)
        .expect("run");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("rule {rule_name:?} missing"))
        .display
        .clone()
        .expect("display")
}

#[test]
fn uses_lemma_units_duration_typedef_and_literals() {
    let code = r#"spec consumer
uses lemma units
data age: units.duration
rule hours: age as hours"#;
    let mut engine = Engine::new();
    engine
        .load(code, path_source("consumer.lemma"))
        .expect("plan");
    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("age".to_string(), "90 minutes".to_string());
    let response = engine
        .run(None, "consumer", Some(&now), data, false, None)
        .expect("run");
    assert_eq!(
        response
            .results
            .get("hours")
            .expect("hours rule")
            .measure
            .as_ref()
            .and_then(|m| m.get("hours"))
            .map(String::as_str),
        Some("1.5")
    );
}

#[test]
fn uses_lemma_units_length_and_duration_units_for_compound_cast() {
    let code = r#"spec speed_test
uses lemma units
data velocity: measure -> unit mps metre/second
data dist: 100 metre
data secs: 20 seconds
rule speed: (dist / secs) as mps"#;
    let out = eval_rule(code, "speed_test", "speed");
    assert!(
        out.contains('5') && out.to_lowercase().contains("mps"),
        "expected 5 mps, got: {out}"
    );
}

#[test]
fn uses_lemma_calendar_does_not_resolve_standalone_spec() {
    let code = r#"spec bad
uses lemma calendar
rule x: 1 year"#;
    let mut engine = Engine::new();
    let err = engine
        .load(code, path_source("bad.lemma"))
        .expect_err("uses lemma calendar must not resolve; use uses lemma units");
    let combined = err
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        !combined.is_empty(),
        "expected planning error for uses lemma calendar, got: {combined:?}"
    );
}

#[test]
fn bare_uses_lemma_without_units_does_not_resolve_stdlib() {
    let code = r#"spec bad
uses lemma
rule x: 1 hour"#;
    let mut engine = Engine::new();
    let err = engine
        .load(code, path_source("bad.lemma"))
        .expect_err("bare uses lemma must not resolve embedded stdlib");
    let combined = err
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        !combined.is_empty(),
        "expected planning error for bare uses lemma, got: {combined:?}"
    );
}
