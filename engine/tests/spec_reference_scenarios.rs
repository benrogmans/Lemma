//! Integration tests locking spec composability behavior (see [spec_composability.md](../../documentation/spec_composability.md)).
//!
//! Each `scenario_XX_*` test encodes a fixture and asserts planning/load or eval outcomes.

use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;
use std::sync::Arc;

fn date(year: i32, month: u32, day: u32) -> DateTimeValue {
    DateTimeValue {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
    }
}

fn path_source(name: &str) -> SourceType {
    SourceType::Path(Arc::new(std::path::PathBuf::from(name)))
}

fn eval(engine: &Engine, spec_name: &str, effective: &DateTimeValue) -> lemma::Response {
    engine
        .run(
            None,
            spec_name,
            Some(effective),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap()
}

fn assert_rule_value(response: &lemma::Response, rule: &str, expected: &str) {
    let result = response
        .results
        .get(rule)
        .unwrap_or_else(|| panic!("rule '{}' not in results", rule));
    let val = result
        .result
        .value()
        .unwrap_or_else(|| panic!("rule '{}' is Veto, expected Value", rule));
    assert_eq!(
        val.to_string(),
        expected,
        "rule '{}': expected {}, got {}",
        rule,
        expected,
        val
    );
}

fn load_err_joined(engine_res: Result<(), lemma::Errors>) -> String {
    let err = engine_res.expect_err("expected load to fail");
    err.errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_temporal_coverage_rejected(joined: &str) {
    assert!(
        joined.contains("no version") || joined.contains("active"),
        "expected temporal coverage wording, got: {joined}"
    );
}

fn assert_self_ref_not_cycle_only(joined: &str) {
    assert!(
        joined.contains("cannot reference itself") && joined.contains("finance"),
        "expected cannot reference itself for finance, got: {joined}"
    );
    assert!(
        !joined.contains("cycle") && !joined.contains("Cycle"),
        "must not fail with cycle-only wording, got: {joined}"
    );
}

// --- Scenario 1: Accepted (unpinned uses parent, temporal slices) ---

#[test]
fn scenario_01_parent_child_unpinned_accepts() {
    let code = r#"
spec parent
rule p: 1

spec parent 2027-01-01
rule p: 2

spec child
uses parent
rule c: parent.p
"#;

    let mut engine = Engine::new();
    engine
        .load(code, path_source("scenario_01.lemma"))
        .expect("scenario 1: unpinned uses parent across slices must plan");

    assert_rule_value(&eval(&engine, "child", &date(2025, 6, 1)), "c", "1");
    assert_rule_value(&eval(&engine, "child", &date(2028, 1, 1)), "c", "2");
}

// --- Scenario 2: Rejected (same-body self-import on finance 2027) ---

#[test]
fn scenario_02_finance_2027_self_import_rejected() {
    let code = r#"
spec finance
data rate: 1

spec finance 2026-01-01
data rate: 2

spec finance 2027-01-01
uses finance
rule ok: finance.rate
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, path_source("scenario_02.lemma")));
    assert_self_ref_not_cycle_only(&joined);
}

// --- Scenario 4: Accepted (qualified pin parent 2025-06-01) ---

#[test]
fn scenario_04_child_pins_parent_2025_06_accepts() {
    let code = r#"
spec parent 2025-01-01
rule p: 1

spec parent 2027-01-01
rule p: 2

spec child
uses parent 2025-06-01
rule c: parent.p
"#;

    let mut engine = Engine::new();
    engine
        .load(code, path_source("scenario_04.lemma"))
        .expect("scenario 4: qualified pin must plan");

    assert_rule_value(&eval(&engine, "child", &date(2025, 3, 1)), "c", "1");
    assert_rule_value(&eval(&engine, "child", &date(2028, 1, 1)), "c", "1");
}

// --- Scenario 13: Rejected (unqualified uses, dep only from 2027) ---

#[test]
fn scenario_13_child_unpinned_parent_only_2027_rejected() {
    let code = r#"
spec parent 2027-01-01
rule p: 1

spec child
uses parent
rule c: parent.p
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, path_source("scenario_13.lemma")));
    assert_temporal_coverage_rejected(&joined);
}

// --- Scenario 14: Accepted (qualified pin to only parent body) ---

#[test]
fn scenario_14_child_pins_parent_2027_accepts() {
    let code = r#"
spec parent 2027-01-01
rule p: 1

spec child
uses parent 2027
rule c: parent.p
"#;

    let mut engine = Engine::new();
    engine
        .load(code, path_source("scenario_14.lemma"))
        .expect("scenario 14: qualified pin to parent 2027 must plan");

    assert_rule_value(&eval(&engine, "child", &date(2025, 3, 1)), "c", "1");
    assert_rule_value(&eval(&engine, "child", &date(2028, 1, 1)), "c", "1");
}

// --- Scenario 15: Accepted (two child slices, both pin parent 2027) ---

#[test]
fn scenario_15_child_slices_both_pin_parent_2027_accepts() {
    let code = r#"
spec parent 2027-01-01
rule p: 1

spec child
uses parent 2027
rule c: parent.p

spec child 2027-01-01
uses parent 2027
rule c: parent.p
"#;

    let mut engine = Engine::new();
    engine
        .load(code, path_source("scenario_15.lemma"))
        .expect("scenario 15: both child rows with pin must plan");

    assert_rule_value(&eval(&engine, "child", &date(2025, 3, 1)), "c", "1");
    assert_rule_value(&eval(&engine, "child", &date(2028, 1, 1)), "c", "1");
}

// --- Scenario 27b: Accepted (fill from nested data, not spec ref) ---

#[test]
fn scenario_27b_fill_copy_from_inner_x_accepts() {
    let code = r#"
spec inner
data x: number -> default 1

spec outer
uses i: inner
fill copy_of_i: i.x
rule r: copy_of_i
"#;

    let mut engine = Engine::new();
    engine
        .load(code, path_source("scenario_27b.lemma"))
        .expect("scenario 27b: fill i.x must plan");

    let now = DateTimeValue::now();
    assert_rule_value(
        &engine
            .run(
                None,
                "outer",
                Some(&now),
                HashMap::new(),
                false,
                lemma::EvaluationRequest::default(),
            )
            .expect("scenario 27b: eval outer"),
        "r",
        "1",
    );
}

// --- Scenario 30: Rejected (unqualified uses, dep only from 2027) ---

#[test]
fn scenario_30_some_unpinned_another_only_2027_rejected() {
    let code = r#"
spec some
uses another
rule y: another.x

spec another 2027-01-01
rule x: 5
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, path_source("scenario_30.lemma")));
    assert_temporal_coverage_rejected(&joined);
}
