//! QA coverage for the `with_data_values` contract on every
//! `DataDefinition` variant.
//!
//! Matrix:
//!   - unknown key → error
//!   - SpecRef → error (no value)
//!   - TypeDeclaration → success
//!   - Value → replaces literal
//!   - Reference → replaces reference target copy
//!   - validation failures per primitive
//!   - related_data attribution in errors

use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use lemma::ErrorKind;
use std::collections::HashMap;

fn rule_value(result: &lemma::evaluation::Response, name: &str) -> String {
    let rr = result
        .results
        .get(name)
        .unwrap_or_else(|| panic!("rule '{}' not found", name));
    match &rr.result {
        OperationResult::Value(v) => v.to_string(),
        OperationResult::Veto(v) => format!("VETO({})", v),
    }
}

#[test]
fn unknown_key_is_rejected() {
    let code = r#"
spec s
data x: number
rule r: x
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("x".to_string(), "1".to_string());
    data.insert("does_not_exist".to_string(), "42".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("unknown key must fail");
    let s = err.to_string();
    assert!(
        s.contains("does_not_exist") || s.contains("not found"),
        "unknown key error must name the key, got: {s}"
    );
}

#[test]
fn override_spec_reference_is_rejected() {
    let code = r#"
spec inner
data x: number -> default 1

spec outer
with i: inner
rule r: i.x
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    // `i` is a SpecRef, not a data value — overriding it is meaningless.
    data.insert("i".to_string(), "42".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("outer", Some(&now), data, false)
        .expect_err("spec-ref override must fail");
    let s = err.to_string();
    assert!(
        s.contains("spec reference") && s.contains("cannot provide"),
        "override on SpecRef must have the exact error pattern, got: {s}"
    );
}

#[test]
fn override_of_type_declaration_succeeds() {
    let code = r#"
spec s
data x: number
rule r: x
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("x".to_string(), "42".to_string());

    let now = DateTimeValue::now();
    let resp = engine.run("s", Some(&now), data, false).expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "42");
}

#[test]
fn override_of_literal_value_replaces() {
    let code = r#"
spec s
data x: 10
rule r: x
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("x".to_string(), "99".to_string());

    let now = DateTimeValue::now();
    let resp = engine.run("s", Some(&now), data, false).expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "99");
}

#[test]
fn override_wrong_primitive_kind_fails_with_related_data() {
    let code = r#"
spec s
data age: number
rule r: age
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("age".to_string(), "thirty".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("wrong kind must fail");

    assert_eq!(
        err.related_data(),
        Some("age"),
        "related_data attribution must point at the failing data name"
    );
    assert_eq!(err.kind(), ErrorKind::Validation);
}

#[test]
fn override_violating_minimum_fails() {
    let code = r#"
spec s
data n: number -> minimum 10
rule r: n
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("n".to_string(), "5".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("violates minimum");
    let s = err.to_string();
    assert!(
        s.contains("minimum") || s.contains("at least"),
        "expected minimum-violation message, got: {s}"
    );
}

#[test]
fn override_violating_maximum_fails() {
    let code = r#"
spec s
data n: number -> maximum 5
rule r: n
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("n".to_string(), "10".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("violates maximum");
    let s = err.to_string();
    assert!(
        s.contains("maximum") || s.contains("at most") || s.contains("exceeds"),
        "expected maximum-violation message, got: {s}"
    );
}

#[test]
fn override_violating_length_fails() {
    let code = r#"
spec s
data msg: text -> length 3
rule r: msg
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("msg".to_string(), "way too long".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("violates length");
    let s = err.to_string();
    assert!(
        s.contains("length"),
        "expected length-violation message, got: {s}"
    );
}

#[test]
fn override_violating_options_fails() {
    // `options` should restrict to a set.
    let code = r#"
spec s
data color: text -> options red green blue
rule r: color
"#;
    let mut engine = Engine::new();
    let load_result = engine.load(code, lemma::SourceType::Labeled("w.lemma"));
    if let Err(errors) = &load_result {
        // If `options` on text is not yet supported, this test pins the gap.
        panic!(
            "`text -> options ...` must be supported or rejected with a clear error at load; \
             got load errors: {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    let mut data = HashMap::new();
    data.insert("color".to_string(), "purple".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("not in options");
    let s = err.to_string();
    assert!(
        s.contains("option") || s.contains("not allowed") || s.contains("valid"),
        "expected options-violation message, got: {s}"
    );
}

#[test]
fn empty_override_map_is_noop() {
    let code = r#"
spec s
data x: 10
rule r: x
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let resp = engine
        .run("s", Some(&now), HashMap::new(), false)
        .expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "10");
}

#[test]
fn override_on_reference_replaces_and_wins_over_target() {
    let code = r#"
spec inner
data v: number -> default 1

spec outer
with i: inner
data copy: i.v
rule r: copy
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("w.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("copy".to_string(), "500".to_string());

    let now = DateTimeValue::now();
    let resp = engine
        .run("outer", Some(&now), data, false)
        .expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "500");
}

#[test]
fn override_on_reference_still_validates_against_merged_type() {
    // Reference LHS declares a tighter max. User provides an out-of-range
    // value. Validation MUST use the merged type (LHS + target + local),
    // not just the target's type.
    let code = r#"
spec inner
data v: number

spec outer
with i: inner
data n: number -> maximum 5
data n: i.v
rule r: n
"#;
    let mut engine = Engine::new();
    let load_result = engine.load(code, lemma::SourceType::Labeled("w.lemma"));
    if load_result.is_err() {
        // Planning might reject this particular shape (LHS+reference redeclare);
        // if so, test ends here (shape-specific error is not our concern).
        return;
    }

    let mut data = HashMap::new();
    data.insert("n".to_string(), "10".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("outer", Some(&now), data, false)
        .expect_err("merged-type validation must reject 10 against LHS `maximum 5`");
    let s = err.to_string();
    assert!(
        s.contains("maximum") || s.contains("at most") || s.contains("exceeds"),
        "merged-type validation should have rejected 10, got: {s}"
    );
}
