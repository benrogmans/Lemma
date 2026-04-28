//! QA coverage for nested LHS paths (`data outer.inner: ...`) — the binding
//! mechanism used to push values into child specs via `with` references.

use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

fn load_ok(engine: &mut Engine, code: &str) {
    engine
        .load(code, lemma::SourceType::Labeled("nested.lemma"))
        .unwrap_or_else(|errs| {
            let joined = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            panic!("expected load to succeed, got: {joined}");
        });
}

fn load_err_joined(engine: &mut Engine, code: &str) -> String {
    let err = engine
        .load(code, lemma::SourceType::Labeled("nested.lemma"))
        .expect_err("expected load to fail");
    err.iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

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

// ─── Happy path: depth 2 and 3 ────────────────────────────────────────

#[test]
fn nested_binding_depth_2_literal() {
    let code = r#"
spec inner
data x: number

spec outer
with i: inner
data i.x: 42
rule r: i.x
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let now = DateTimeValue::now();
    let resp = engine
        .run("outer", Some(&now), HashMap::new(), false)
        .expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "42");
}

#[test]
fn nested_binding_depth_3_literal() {
    let code = r#"
spec leaf
data v: number

spec middle
with l: leaf

spec outer
with m: middle
data m.l.v: 7
rule r: m.l.v
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let now = DateTimeValue::now();
    let resp = engine
        .run("outer", Some(&now), HashMap::new(), false)
        .expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "7");
}

// ─── Error cases: structural ─────────────────────────────────────────

#[test]
fn binding_where_first_segment_is_not_spec_ref_is_rejected() {
    let code = r#"
spec s
data x: number -> default 1
data x.y: 42
rule r: x
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        joined.contains("not a spec reference") || joined.contains("is not a spec reference"),
        "binding through non-spec-ref first segment must be rejected, got: {joined}"
    );
}

#[test]
fn binding_targeting_nonexistent_child_data_is_rejected() {
    let code = r#"
spec inner
data x: number

spec outer
with i: inner
data i.nonexistent: 42
rule r: i.x
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        joined.contains("nonexistent")
            || joined.contains("does not exist")
            || joined.contains("not found"),
        "binding to non-existent child data must be rejected and mention the name, got: {joined}"
    );
}

#[test]
fn duplicate_binding_is_rejected_with_previous_location() {
    let code = r#"
spec inner
data x: number

spec outer
with i: inner
data i.x: 1
data i.x: 2
rule r: i.x
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        joined.contains("Duplicate")
            || joined.contains("duplicate")
            || joined.contains("previously"),
        "duplicate binding must be rejected and reference prior location, got: {joined}"
    );
}

#[test]
fn binding_rhs_as_type_declaration_is_rejected() {
    // Binding to a child data with `number` (type decl) is semantically
    // wrong — bindings must supply VALUES, not typedefs.
    let code = r#"
spec inner
data x: number

spec outer
with i: inner
data i.x: number
rule r: i.x
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        joined.contains("literal value") || joined.contains("type declaration"),
        "binding with type declaration must be rejected, got: {joined}"
    );
}

#[test]
fn binding_rhs_as_spec_reference_is_rejected() {
    // `data i.x: spec something` is the legacy removed syntax. Must fail.
    let code = r#"
spec other
data y: number -> default 1

spec inner
data x: number

spec outer
with i: inner
with o: other
data i.x: spec other
rule r: i.x
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        joined.contains("spec") || joined.contains("removed") || joined.contains("syntax"),
        "binding RHS as spec keyword must be rejected (legacy syntax removed), got: {joined}"
    );
}

// ─── User override via with_data_values uses dotted input_key ────────

#[test]
fn user_override_of_nested_binding_via_dotted_key() {
    let code = r#"
spec inner
data x: number

spec outer
with i: inner
data i.x: 42
rule r: i.x
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("i.x".to_string(), "99".to_string());
    let now = DateTimeValue::now();
    let resp = engine
        .run("outer", Some(&now), data, false)
        .expect("evaluates");
    assert_eq!(
        rule_value(&resp, "r"),
        "99",
        "user override via 'i.x' dotted key must win over binding literal"
    );
}

#[test]
fn user_override_of_depth_3_binding() {
    let code = r#"
spec leaf
data v: number

spec middle
with l: leaf

spec outer
with m: middle
data m.l.v: 5
rule r: m.l.v
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("m.l.v".to_string(), "123".to_string());
    let now = DateTimeValue::now();
    let resp = engine
        .run("outer", Some(&now), data, false)
        .expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "123");
}

// ─── Override key casing: exact-match only ───────────────────────────

#[test]
fn user_override_key_is_case_sensitive() {
    // `X` is not the same as `x`. Case-insensitive matching would be a
    // bug — silently overriding the wrong field is dangerous.
    let code = r#"
spec s
data x: number -> default 1
rule r: x
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("X".to_string(), "99".to_string());
    let now = DateTimeValue::now();
    let result = engine.run("s", Some(&now), data, false);
    assert!(
        result.is_err(),
        "uppercase 'X' must not match 'x'; engine silently accepted case-insensitive key"
    );
}
