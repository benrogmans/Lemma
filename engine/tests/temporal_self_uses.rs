//! Cross-temporal same-name `uses`: a later `spec finance …` body may depend on an
//! earlier body via `uses finance 2026` or `uses fin: finance 2026`.

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

fn eval(engine: &Engine, spec_name: &str, effective: &DateTimeValue) -> lemma::Response {
    engine
        .run(None, spec_name, Some(effective), HashMap::new(), false)
        .unwrap()
}

fn assert_rule_value(response: &lemma::Response, rule: &str, expected: &str) {
    let result = response
        .results
        .get(rule)
        .unwrap_or_else(|| panic!("rule '{}' not in results", rule));
    if result.vetoed {
        panic!(
            "rule '{}' is Veto: {}",
            rule,
            result.veto_reason.as_deref().unwrap_or("Vetoed")
        );
    }
    assert_eq!(
        result.display.as_deref(),
        Some(expected),
        "rule '{}': expected {}, got {:?}",
        rule,
        expected,
        result.display
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

fn assert_forward_pin_rejected(joined: &str) {
    assert!(
        joined.contains("cannot reference itself")
            || joined.contains("active at that instant")
            || joined.contains("does not exist")
            || joined.contains("depends on"),
        "expected missing-slice or self-reference planning error, got: {joined}"
    );
    assert!(
        !joined.contains("cycle") && !joined.contains("Cycle"),
        "forward pin must not be rejected by cycle-only wording, got: {joined}"
    );
}

fn load_finance_origin_and_consumer(
    engine: &mut Engine,
    consumer_body: &str,
) -> Result<(), lemma::Errors> {
    engine.load(
        r#"
spec finance
data rate: number -> default 0
"#,
        SourceType::Path(Arc::new(std::path::PathBuf::from("finance_origin.lemma"))),
    )?;
    engine.load(
        consumer_body,
        SourceType::Path(Arc::new(std::path::PathBuf::from("finance_consumer.lemma"))),
    )
}

fn load_finance_specs(engine: &mut Engine, consumer_body: &str) {
    engine
        .load(
            r#"
spec finance 2026-01-01
data rate: 1

spec finance 2024-01-01
data rate: 2
"#,
            SourceType::Path(Arc::new(std::path::PathBuf::from("finance_base.lemma"))),
        )
        .expect("base finance temporal rows");

    engine
        .load(
            consumer_body,
            SourceType::Path(Arc::new(std::path::PathBuf::from("finance_consumer.lemma"))),
        )
        .expect("consumer spec");
}

#[test]
fn temporal_self_uses_without_alias() {
    let mut engine = Engine::new();
    load_finance_specs(
        &mut engine,
        r#"
spec finance 2027-01-01
uses f26: finance 2026-01-01
rule ok: f26.rate
"#,
    );

    assert_rule_value(&eval(&engine, "finance", &date(2027, 1, 15)), "ok", "1");
}

#[test]
fn temporal_self_uses_with_explicit_alias() {
    let mut engine = Engine::new();
    load_finance_specs(
        &mut engine,
        r#"
spec finance 2026-05-20
uses fin: finance 2024-01-01
rule ok: fin.rate
"#,
    );

    assert_rule_value(&eval(&engine, "finance", &date(2026, 5, 20)), "ok", "2");
}

#[test]
fn true_self_uses_on_origin_spec_is_rejected() {
    let mut engine = Engine::new();
    let err = engine
        .load(
            r#"
spec finance
data rate: 1

uses finance
rule ok: finance.rate
"#,
            SourceType::Volatile,
        )
        .unwrap_err();

    let joined = err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.contains("cannot reference itself") && joined.contains("finance"),
        "expected true self-reference, got: {joined}"
    );
}

#[test]
fn true_self_uses_implicit_alias_on_origin_is_rejected() {
    let mut engine = Engine::new();
    let err = engine
        .load(
            "spec finance\nuses finance\nrule ok: 1",
            SourceType::Volatile,
        )
        .unwrap_err();

    let joined = err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.contains("cannot reference itself"),
        "expected true self-reference, got: {joined}"
    );
}

#[test]
fn true_self_rejects_cannot_reference_itself() {
    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(
        r#"
spec finance
data rate: 1

uses finance
rule ok: finance.rate
"#,
        SourceType::Volatile,
    ));
    assert!(
        joined.contains("cannot reference itself") && joined.contains("finance"),
        "expected cannot reference itself, got: {joined}"
    );
}

#[test]
fn forward_pin_no_matching_slice_explicit_alias() {
    let mut engine = Engine::new();
    let joined = load_err_joined(load_finance_origin_and_consumer(
        &mut engine,
        r#"
spec finance 2026-05-20
uses fin: finance 2027
"#,
    ));
    assert_forward_pin_rejected(&joined);
}

#[test]
fn forward_pin_no_matching_slice_implicit_alias() {
    let mut engine = Engine::new();
    let joined = load_err_joined(load_finance_origin_and_consumer(
        &mut engine,
        r#"
spec finance 2026-05-20
uses finance 2027
"#,
    ));
    assert_forward_pin_rejected(&joined);
}

#[test]
fn forward_pin_with_rules_not_cycle_only() {
    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(
        r#"
spec finance
data rate: number -> default 0

spec finance 2026-05-20
uses fin: finance 2027
rule x: 1

spec finance 2024-01-01
uses prev: finance 2026-05-20
rule y: prev.x
"#,
        SourceType::Volatile,
    ));
    assert!(
        joined.contains("2026") || joined.contains("2027") || joined.contains("not active at"),
        "consumer or pin context expected, got: {joined}"
    );
    assert!(
        joined.contains("cannot reference itself")
            || joined.contains("active at that instant")
            || joined.contains("depends on"),
        "expected missing-slice or self-reference, got: {joined}"
    );
    assert!(
        !(joined.contains("cycle") || joined.contains("Cycle"))
            || joined.contains("cannot reference itself")
            || joined.contains("not active at"),
        "must not fail with cycle only, got: {joined}"
    );
}

#[test]
fn forward_pin_missing_year_body() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec finance 2026-01-01
data rate: 1

spec finance 2024-01-01
data rate: 2
"#,
            SourceType::Volatile,
        )
        .expect("base rows");
    let joined = load_err_joined(engine.load(
        r#"
spec finance 2026-05-20
uses fin: finance 2028
"#,
        SourceType::Volatile,
    ));
    assert_forward_pin_rejected(&joined);
}

#[test]
fn temporal_self_uses_three_slice_chain() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec finance 2024-01-01
data rate: 2

spec finance 2026-01-01
data rate: 1

spec finance 2027-01-01
uses f26: finance 2026-01-01
rule from_2026: f26.rate
"#,
            SourceType::Volatile,
        )
        .expect("2027 slice");

    engine
        .load(
            r#"
spec finance 2026-05-20
uses fin: finance 2024-01-01
rule from_2024: fin.rate
"#,
            SourceType::Volatile,
        )
        .expect("2026 consumer");

    assert_rule_value(
        &eval(&engine, "finance", &date(2027, 1, 15)),
        "from_2026",
        "1",
    );
    assert_rule_value(
        &eval(&engine, "finance", &date(2026, 5, 20)),
        "from_2024",
        "2",
    );
}

#[test]
fn same_instant_pin_is_rejected() {
    let mut engine = Engine::new();
    let err = engine
        .load(
            r#"
spec finance 2026-01-01
data rate: 1

uses finance 2026-01-01
rule ok: finance.rate
"#,
            SourceType::Volatile,
        )
        .unwrap_err();

    let joined = err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.contains("cannot reference itself"),
        "expected same-body pin error, got: {joined}"
    );
}

#[test]
fn temporal_self_uses_cycle_is_rejected() {
    let mut engine = Engine::new();
    let err = engine
        .load(
            r#"
spec finance 2026-01-01
uses f27: finance 2027-01-01
rule a: 1

spec finance 2027-01-01
uses f26: finance 2026-01-01
rule b: 1
"#,
            SourceType::Volatile,
        )
        .unwrap_err();

    let joined = err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.contains("cycle") || joined.contains("Cycle"),
        "expected dependency cycle, got: {joined}"
    );
}
