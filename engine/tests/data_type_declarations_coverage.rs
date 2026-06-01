//! QA coverage for parse-time `DataValue::Definition` (schema / constraints / literals).
//!
//! Matrix: every primitive keyword x applicable-vs-incompatible constraint.
//! Named-typedef references: happy + unknown + name-collision with rule.
//! Qualified parent types (`uses` plus `data x: alias.TypeName`): happy + unknown + errors.
//!
//! Tests that encode INTENDED behavior stay as written. Several are expected
//! to be red (e.g. `text -> decimals 2` may be silently accepted). Do NOT
//! mask, delete, or weaken.

use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

fn load_ok(engine: &mut Engine, code: &str) {
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("types.lemma"))),
        )
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
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("types.lemma"))),
        )
        .expect_err("expected load to fail");
    err.iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn rule_value(result: &lemma::Response, name: &str) -> String {
    let rr = result
        .results
        .get(name)
        .unwrap_or_else(|| panic!("rule '{}' not found", name));
    if rr.vetoed {
        format!("VETO({})", rr.veto_reason.as_deref().unwrap_or("Vetoed"))
    } else {
        rr.display.clone().expect("display")
    }
}

fn run(
    engine: &Engine,
    spec: &str,
    data: HashMap<String, String>,
) -> Result<lemma::Response, lemma::Error> {
    let now = DateTimeValue::now();
    engine.run(None, spec, Some(&now), data, false)
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
        rr.vetoed,
        "type-only number data missing at runtime must produce MissingData veto, got: {:?}",
        rr.veto_reason
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
        rr.vetoed,
        "type-only text missing must produce MissingData veto, got: {:?}",
        rr.veto_reason
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
    assert!(rr.vetoed, "got: {:?}", rr.veto_reason);
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
    assert!(rr.vetoed, "got: {:?}", rr.veto_reason);
}

#[test]
fn lemma_typedef_duration_missing_data_vetoes() {
    let code = r#"
spec s
uses lemma units
data d: units.duration
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s", HashMap::new()).expect("evaluates");
    let rr = resp.results.get("r").unwrap();
    assert!(rr.vetoed, "got: {:?}", rr.veto_reason);
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
    assert!(rr.vetoed, "got: {:?}", rr.veto_reason);
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
    let resp = run(&engine, "s", data).expect("5 < 10 must complete with veto");
    let rr = resp.results.get("r").expect("rule r");
    assert!(rr.vetoed, "5 < 10 must veto rule r");
    let reason = rr.veto_reason.as_deref().expect("veto reason");
    assert!(
        reason.contains("minimum") || reason.contains("at least"),
        "expected minimum constraint veto, got: {reason}"
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
    let resp = run(&engine, "s", data).expect("10 > 5 must complete with veto");
    let rr = resp.results.get("r").expect("rule r");
    assert!(rr.vetoed, "10 > 5 must veto rule r");
    let reason = rr.veto_reason.as_deref().expect("veto reason");
    assert!(
        reason.contains("maximum") || reason.contains("at most") || reason.contains("exceeds"),
        "expected maximum constraint veto, got: {reason}"
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
    let resp = run(&engine, "s", data).expect("length 5 must complete with veto");
    let rr = resp.results.get("r").expect("rule r");
    assert!(rr.vetoed, "length 5 must reject longer text");
    let reason = rr.veto_reason.as_deref().expect("veto reason");
    assert!(
        reason.contains("length"),
        "expected length constraint veto, got: {reason}"
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
    match engine.load(
        code,
        lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("types.lemma"))),
    ) {
        Ok(()) => {
            let mut data = HashMap::new();
            data.insert("n".to_string(), "7".to_string());
            let resp = run(&engine, "s", data)
                .expect("either last-wins (7<10 veto) or this case should have been a plan error");
            let rr = resp.results.get("r").expect("rule r");
            assert!(rr.vetoed, "7 < effective minimum 10 must veto rule r");
            let reason = rr.veto_reason.as_deref().expect("veto reason");
            assert!(
                reason.contains("minimum") || reason.contains("at least"),
                "expected minimum violation; got: {reason}"
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
    let resp = run(&engine, "s", data).expect("200 > 150 must complete with veto");
    let rr = resp.results.get("r").expect("rule r");
    assert!(rr.vetoed, "200 > 150 must veto via inherited max");
    let reason = rr.veto_reason.as_deref().expect("veto reason");
    assert!(
        reason.contains("maximum") || reason.contains("150") || reason.contains("at most"),
        "expected inherited max veto; got: {reason}"
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
        joined.contains("Unknown parent") && joined.contains("nonexistent_type"),
        "unknown typedef must be reported with exact name, got: {joined}"
    );
}

/// UX LANDMINE: `data x: myrule` where `myrule` is a local rule currently
/// surfaces as "Unknown parent … for data definition". Users likely meant a value-copy reference.
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
        "error for `data x: <rule-name>` should mention rules/references; got: {joined}"
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
    let resp = run(&engine, "s", data).expect("narrowed max 100 must complete with veto");
    let rr = resp.results.get("r").expect("rule r");
    assert!(rr.vetoed, "narrowed max 100 must reject 200");
    let reason = rr.veto_reason.as_deref().expect("veto reason");
    assert!(
        reason.contains("maximum") || reason.contains("100") || reason.contains("at most"),
        "narrowed max must veto; got: {reason}"
    );
}

#[test]
fn cross_spec_value_copy_reference_resolves() {
    let code = r#"
spec lib
data money: quantity -> unit eur 1 -> unit usd 0.84

spec app
uses lib
data price: lib.money
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
fn cross_spec_value_copy_unknown_data_is_rejected() {
    let code = r#"
spec lib
data money: quantity -> unit eur 1

spec app
uses lib
with lib.money: lib.nonexistent
rule r: lib.money
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty() && (joined.contains("nonexistent") || joined.contains("not found")),
        "unknown data in cross-spec value-copy reference must be rejected, got: {joined}"
    );
}

#[test]
fn cross_spec_value_copy_to_unknown_spec_is_rejected() {
    let code = r#"
spec app
uses dep: nonexistent_spec
with dep.money: 1
rule r: dep.money
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty(),
        "unknown spec in `uses` must be rejected"
    );
}
