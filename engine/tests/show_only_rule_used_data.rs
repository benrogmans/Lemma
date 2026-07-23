//! Show must list only data used by the spec's rules.

use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::BTreeSet;

#[test]
fn show_lists_only_data_used_by_rules() {
    let code = r#"
spec t
data used: 4
data unused: 5
rule r: used
"#;

    let mut engine = Engine::new();
    engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .expect("load");
    let show = engine
        .show(None, "t", Some(&DateTimeValue::now()))
        .expect("show");

    let keys: BTreeSet<&str> = show.data.keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["used"]));
}

#[test]
fn show_omits_data_only_used_in_primary_arm_under_unless_true() {
    let code = r#"
spec t
data used: 4
data dead: 5
rule r: used * dead
 unless true then 1
"#;

    let mut engine = Engine::new();
    engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .expect("load");
    let show = engine
        .show(None, "t", Some(&DateTimeValue::now()))
        .expect("show");

    let keys: BTreeSet<&str> = show.data.keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::new());
}

#[test]
fn show_omits_dead_under_unless_inlined_divide_rule() {
    let code = r#"
spec t
data dead: 5
rule dep: 5 / 2
rule r: dead
 unless dep < 4 then 1
"#;

    let mut engine = Engine::new();
    engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .expect("load");
    let show = engine
        .show(None, "t", Some(&DateTimeValue::now()))
        .expect("show");

    assert!(
        !show.data.contains_key("dead"),
        "dead must not appear when unless dep < 4 is statically true; keys: {:?}",
        show.data.keys().collect::<Vec<_>>()
    );
}

#[test]
fn show_omits_dead_under_unless_nested_arithmetic() {
    let code = r#"
spec t
data dead: 5
rule r: dead
 unless (1 + 2) * 3 / 4 < 10 then 1
"#;

    let mut engine = Engine::new();
    engine
        .load([(lemma::SourceType::Volatile, code.to_string())])
        .expect("load");
    let show = engine
        .show(None, "t", Some(&DateTimeValue::now()))
        .expect("show");

    assert!(
        !show.data.contains_key("dead"),
        "dead must not appear when nested arithmetic unless is statically true; keys: {:?}",
        show.data.keys().collect::<Vec<_>>()
    );
}
