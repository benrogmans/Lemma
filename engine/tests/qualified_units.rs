//! Optionally qualify units; must qualify when ambiguous.

use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn path_source(file: &str) -> SourceType {
    SourceType::Path(Arc::new(PathBuf::from(file)))
}

fn eval_display(code: &str, spec: &str, rule: &str) -> String {
    let mut engine = Engine::new();
    engine
        .load([(path_source("qualified_units.lemma"), code.to_string())])
        .expect("load");
    let now = DateTimeValue::now();
    let response = engine
        .run(None, spec, Some(&now), HashMap::new(), None, false)
        .expect("run");
    response
        .results
        .get(rule)
        .unwrap_or_else(|| panic!("rule {rule}"))
        .display()
        .expect("display")
        .to_string()
}

fn expect_plan_error(code: &str) -> String {
    let mut engine = Engine::new();
    let err = engine
        .load([(path_source("bad.lemma"), code.to_string())])
        .expect_err("expected planning error");
    err.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn optional_qualify_when_unique() {
    let code = r#"spec t
uses lemma units
rule a: 5 kilogram
rule b: 5 units.kilogram
rule c: 5 units.mass.kilogram"#;
    assert!(eval_display(code, "t", "a").contains("kilogram"));
    assert!(eval_display(code, "t", "b").contains("kilogram"));
    assert!(eval_display(code, "t", "c").contains("kilogram"));
}

#[test]
fn bare_ambiguous_must_qualify() {
    let msg = expect_plan_error(
        r#"spec t
data money_a: measure -> unit eur: 1
data money_b: measure -> unit eur: 2
rule r: 1 eur"#,
    );
    assert!(
        msg.to_lowercase().contains("ambiguous")
            || msg.contains("Qualify")
            || msg.contains("qualify"),
        "expected ambiguous bare eur error, got: {msg}"
    );
    assert!(
        msg.contains("money_a.eur") && msg.contains("money_b.eur"),
        "got: {msg}"
    );
}

#[test]
fn qualified_disambiguation_and_eval_arithmetic() {
    let code = r#"spec t
data money_a: measure -> unit eur: 1
data money_b: measure -> unit eur: 2
rule a: 10 money_a.eur + 5 money_a.eur
rule b: 10 money_b.eur + 5 money_b.eur"#;
    let a = eval_display(code, "t", "a");
    let b = eval_display(code, "t", "b");
    assert!(a.contains("15") && a.contains("eur"), "got {a}");
    assert!(b.contains("15") && b.contains("eur"), "got {b}");
}

#[test]
fn import_and_local_kilogram_clash() {
    let bare = expect_plan_error(
        r#"spec t
uses lemma units
data bag: measure -> unit kilogram: 999
rule r: 1 kilogram"#,
    );
    assert!(
        bare.to_lowercase().contains("ambiguous")
            || bare.contains("Qualify")
            || bare.contains("qualify"),
        "got: {bare}"
    );

    let code = r#"spec t
uses lemma units
data bag: measure -> unit kilogram: 999
rule si: 1 units.mass.kilogram
rule local: 1 bag.kilogram
rule sugar: 1 units.kilogram"#;
    assert!(eval_display(code, "t", "si").contains("kilogram"));
    assert!(eval_display(code, "t", "local").contains("kilogram"));
    assert!(eval_display(code, "t", "sugar").contains("kilogram"));
}

#[test]
fn as_qualified_unit() {
    let code = r#"spec t
data money_a: measure -> unit eur: 1
data money_b: measure -> unit eur: 2
data x: 10 money_a.eur
rule r: x as money_a.eur"#;
    assert!(eval_display(code, "t", "r").contains("eur"));
}

/// Multi-owner bare `second` (local + lemma units) must not panic when datetime
/// arithmetic expands an anonymous duration signature without typed owners.
#[test]
fn multi_owner_second_datetime_subtract_does_not_panic() {
    let code = r#"spec t
uses lemma units
data tick: measure -> unit second: 1
rule value: (2024-01-01T03:30:00Z - 01:00:00) as minute"#;
    let display = eval_display(code, "t", "value");
    assert!(
        display.contains("150") && display.contains("minute"),
        "got {display}"
    );
}
