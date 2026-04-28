//! QA coverage for `DataValue::TypeDeclaration`.
//!
//! Matrix: every primitive keyword x applicable-vs-incompatible constraint.
//! Named-typedef references: happy + unknown + name-collision with rule.
//! `from <spec>` variants: happy + unknown typedef + unknown spec.
//!
//! Tests that encode INTENDED behavior stay as written. Several are expected
//! to be red (e.g. `text -> decimals 2` may be silently accepted). Do NOT
//! mask, delete, or weaken.

use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

fn load_ok(engine: &mut Engine, code: &str) {
    engine
        .load(code, lemma::SourceType::Labeled("types.lemma"))
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
        .load(code, lemma::SourceType::Labeled("types.lemma"))
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

fn run(
    engine: &Engine,
    spec: &str,
    data: HashMap<String, String>,
) -> Result<lemma::evaluation::Response, lemma::Error> {
    let now = DateTimeValue::now();
    engine.run(spec, Some(&now), data, false)
}

// ─── Type-only data + missing at runtime → MissingData veto ──────────

#[test]
fn primitive_number_type_only_missing_vetoes() {
    let code = r#"
spec s
data x: number
rule r: x
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s", HashMap::new()).expect("evaluates");
    let rr = resp.results.get("r").unwrap();
    assert!(
        matches!(
            rr.result,
            OperationResult::Veto(lemma::VetoType::MissingData { .. })
        ),
        "type-only number data missing at runtime must produce MissingData veto, got: {:?}",
        rr.result
    );
}

#[test]
fn primitive_text_type_only_missing_vetoes() {
    let code = r#"
spec s
data x: text
rule r: x
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s", HashMap::new()).expect("evaluates");
    let rr = resp.results.get("r").unwrap();
    assert!(
        matches!(
            rr.result,
            OperationResult::Veto(lemma::VetoType::MissingData { .. })
        ),
        "type-only text missing must produce MissingData veto, got: {:?}",
        rr.result
    );
}

#[test]
fn primitive_boolean_type_only_missing_vetoes() {
    let code = r#"
spec s
data b: boolean
rule r: b
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s", HashMap::new()).expect("evaluates");
    let rr = resp.results.get("r").unwrap();
    assert!(
        matches!(
            rr.result,
            OperationResult::Veto(lemma::VetoType::MissingData { .. })
        ),
        "got: {:?}",
        rr.result
    );
}

#[test]
fn primitive_date_type_only_missing_vetoes() {
    let code = r#"
spec s
data d: date
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s", HashMap::new()).expect("evaluates");
    let rr = resp.results.get("r").unwrap();
    assert!(
        matches!(
            rr.result,
            OperationResult::Veto(lemma::VetoType::MissingData { .. })
        ),
        "got: {:?}",
        rr.result
    );
}

#[test]
fn primitive_duration_type_only_missing_vetoes() {
    let code = r#"
spec s
data d: duration
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s", HashMap::new()).expect("evaluates");
    let rr = resp.results.get("r").unwrap();
    assert!(
        matches!(
            rr.result,
            OperationResult::Veto(lemma::VetoType::MissingData { .. })
        ),
        "got: {:?}",
        rr.result
    );
}

#[test]
fn primitive_percent_type_only_missing_vetoes() {
    let code = r#"
spec s
data p: percent
rule r: p
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s", HashMap::new()).expect("evaluates");
    let rr = resp.results.get("r").unwrap();
    assert!(
        matches!(
            rr.result,
            OperationResult::Veto(lemma::VetoType::MissingData { .. })
        ),
        "got: {:?}",
        rr.result
    );
}

// ─── Constraint × primitive compatibility matrix ─────────────────────

// `minimum` / `maximum` on number: valid, enforced
#[test]
fn number_minimum_enforces_on_user_value() {
    let code = r#"
spec s
data n: number -> minimum 10
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("n".to_string(), "5".to_string());
    let err = run(&engine, "s", data).expect_err("5 < 10 must be rejected");
    assert!(
        err.to_string().contains("minimum") || err.to_string().contains("at least"),
        "expected minimum constraint error, got: {}",
        err
    );
}

#[test]
fn number_maximum_enforces_on_user_value() {
    let code = r#"
spec s
data n: number -> maximum 5
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("n".to_string(), "10".to_string());
    let err = run(&engine, "s", data).expect_err("10 > 5 must be rejected");
    let s = err.to_string();
    assert!(
        s.contains("maximum") || s.contains("at most") || s.contains("exceeds"),
        "expected maximum constraint error, got: {s}"
    );
}

// `minimum` on text: INCOMPATIBLE — must be rejected at plan time
#[test]
fn text_minimum_constraint_is_rejected() {
    let code = r#"
spec s
data x: text -> minimum 5
rule r: x
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty() && (joined.contains("minimum") || joined.contains("text")),
        "text does not support `minimum`; must be rejected, got: {joined}"
    );
}

// `decimals` on boolean: INCOMPATIBLE
#[test]
fn boolean_decimals_constraint_is_rejected() {
    let code = r#"
spec s
data b: boolean -> decimals 2
rule r: b
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty() && (joined.contains("decimals") || joined.contains("boolean")),
        "boolean does not support `decimals`; must be rejected, got: {joined}"
    );
}

// `decimals` on text: INCOMPATIBLE
#[test]
fn text_decimals_constraint_is_rejected() {
    let code = r#"
spec s
data x: text -> decimals 2
rule r: x
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty() && (joined.contains("decimals") || joined.contains("text")),
        "text does not support `decimals`; must be rejected, got: {joined}"
    );
}

// `unit` on date: INCOMPATIBLE
#[test]
fn date_unit_constraint_is_rejected() {
    let code = r#"
spec s
data d: date -> unit meter 1
rule r: d
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty() && (joined.contains("unit") || joined.contains("date")),
        "date does not support `unit`; must be rejected, got: {joined}"
    );
}

// `length` on text: VALID
#[test]
fn text_length_constraint_enforces_on_user_value() {
    let code = r#"
spec s
data msg: text -> length 5
rule r: msg
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("msg".to_string(), "way too long".to_string());
    let err = run(&engine, "s", data).expect_err("length 5 must reject longer text");
    assert!(
        err.to_string().contains("length"),
        "expected length constraint error, got: {}",
        err
    );
}

// `length` on number: INCOMPATIBLE
#[test]
fn number_length_constraint_is_rejected() {
    let code = r#"
spec s
data n: number -> length 5
rule r: n
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty() && (joined.contains("length") || joined.contains("number")),
        "number does not support `length`; must be rejected, got: {joined}"
    );
}

// ─── Default constraint ──────────────────────────────────────────────

#[test]
fn default_constraint_supplies_value_when_missing() {
    let code = r#"
spec s
data n: number -> default 42
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s", HashMap::new()).expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "42");
}

#[test]
fn default_is_overridden_by_user_value() {
    let code = r#"
spec s
data n: number -> default 42
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("n".to_string(), "99".to_string());
    let resp = run(&engine, "s", data).expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "99");
}

#[test]
fn default_that_violates_sibling_constraint_is_rejected() {
    // Default 3 violates `minimum 5` on the same chain.
    let code = r#"
spec s
data n: number -> default 3 -> minimum 5
rule r: n
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty()
            && (joined.contains("default") || joined.contains("minimum") || joined.contains("3")),
        "default violating minimum on same chain must be rejected, got: {joined}"
    );
}

#[test]
fn default_of_wrong_primitive_is_rejected() {
    let code = r#"
spec s
data n: number -> default "not a number"
rule r: n
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty(),
        "default of wrong primitive must be rejected; engine silently accepted it"
    );
}

// ─── Chained constraints ─────────────────────────────────────────────

#[test]
fn chained_tightening_minimum_is_consistent() {
    // `-> minimum 5 -> minimum 10` — pin behavior. Either last-wins
    // (effective min 10) or plan error. Silent loss is wrong.
    let code = r#"
spec s
data n: number -> minimum 5 -> minimum 10
rule r: n
"#;
    let mut engine = Engine::new();
    match engine.load(code, lemma::SourceType::Labeled("types.lemma")) {
        Ok(()) => {
            let mut data = HashMap::new();
            data.insert("n".to_string(), "7".to_string());
            let err = run(&engine, "s", data).expect_err(
                "either last-wins (7<10 rejected) or this case should have been a plan error",
            );
            let s = err.to_string();
            assert!(
                s.contains("minimum") || s.contains("at least"),
                "expected minimum violation; got: {s}"
            );
        }
        Err(errs) => {
            let joined = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                !joined.is_empty(),
                "rejection must carry a message, not empty errors"
            );
        }
    }
}

// ─── Named typedef reference ─────────────────────────────────────────

#[test]
fn typedef_reference_resolves() {
    let code = r#"
spec s
data age: number -> minimum 0 -> maximum 150
data person_age: age
rule r: person_age
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("person_age".to_string(), "30".to_string());
    let resp = run(&engine, "s", data).expect("evaluates");
    assert_eq!(rule_value(&resp, "r"), "30");
}

#[test]
fn typedef_reference_inherits_constraints() {
    let code = r#"
spec s
data age: number -> minimum 0 -> maximum 150
data person_age: age
rule r: person_age
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("person_age".to_string(), "200".to_string());
    let err = run(&engine, "s", data).expect_err("200 > 150 must be rejected via inherited max");
    let s = err.to_string();
    assert!(
        s.contains("maximum") || s.contains("150") || s.contains("at most"),
        "expected inherited max to reject; got: {s}"
    );
}

#[test]
fn typedef_reference_to_unknown_name_is_rejected() {
    let code = r#"
spec s
data x: nonexistent_type
rule r: x
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        joined.contains("Unknown type") && joined.contains("nonexistent_type"),
        "unknown typedef must be reported with exact name, got: {joined}"
    );
}

/// UX LANDMINE: `data x: myrule` where `myrule` is a local rule currently
/// surfaces as "Unknown type". Users likely meant a value-copy reference.
/// The error should mention the rule or suggest `x.something: myrule`
/// binding form. This test pins the friendlier-error intent; it is expected
/// to fail until the planner suggests the rule alternative.
#[test]
fn data_referencing_local_rule_name_suggests_reference_syntax() {
    let code = r#"
spec s
rule myrule: 42
data x: myrule
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        joined.to_lowercase().contains("rule"),
        "error for `data x: <rule-name>` should mention rules/references, not only 'Unknown type'; \
         got: {joined}"
    );
}

#[test]
fn typedef_chain_narrowing_child_constraints_is_ok() {
    let code = r#"
spec s
data big_number: number -> minimum 0 -> maximum 1000
data small_number: big_number -> maximum 100
rule r: small_number
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("small_number".to_string(), "200".to_string());
    let err = run(&engine, "s", data).expect_err("narrowed max 100 must reject 200");
    let s = err.to_string();
    assert!(
        s.contains("maximum") || s.contains("100") || s.contains("at most"),
        "narrowed max must reject; got: {s}"
    );
}

// ─── `from <spec>` RHS type import ──────────────────────────────────

#[test]
fn from_spec_type_import_resolves() {
    let code = r#"
spec lib
data money: scale -> unit eur 1 -> unit usd 1.19

spec app
data price: money from lib
rule r: price
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("price".to_string(), "100 eur".to_string());
    let resp = run(&engine, "app", data).expect("evaluates");
    let out = rule_value(&resp, "r");
    assert!(out.contains("100") && out.contains("eur"), "got: {out}");
}

#[test]
fn from_spec_with_unknown_typedef_is_rejected() {
    let code = r#"
spec lib
data money: scale -> unit eur 1

spec app
data price: nonexistent from lib
rule r: price
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty() && (joined.contains("nonexistent") || joined.contains("Unknown")),
        "unknown typedef in `from lib` must be rejected, got: {joined}"
    );
}

#[test]
fn from_unknown_spec_is_rejected() {
    let code = r#"
spec app
data price: money from nonexistent_spec
rule r: price
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty(),
        "unknown spec in `from` must be rejected"
    );
}
