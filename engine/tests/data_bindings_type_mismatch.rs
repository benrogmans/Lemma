//! Regression: with bindings with wrong literal shape must return planning errors, not panic.

use lemma::Engine;
use std::path::PathBuf;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("data_bindings_type_mismatch.lemma")))
}

fn load_err_joined(engine: &mut Engine, code: &str) -> String {
    let err = engine
        .load([(source(), code.to_string())])
        .expect_err("expected load to fail");
    err.iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn load_ok(engine: &mut Engine, code: &str) {
    engine
        .load([(source(), code.to_string())])
        .unwrap_or_else(|errs| {
            let joined = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            panic!("expected load to succeed, got: {joined}");
        });
}

const INNER_SPEC: &str = r#"spec product_structure
data primary_weight: measure
  -> unit kilogram: 1
  -> minimum 0 kilogram
"#;

#[test]
fn fill_bare_number_into_measure_slot_returns_planning_error() {
    let code = format!(
        r#"{INNER_SPEC}
spec almonds
uses product_structure
  -> with primary_weight: 10
"#
    );
    let mut engine = Engine::new();
    let err = load_err_joined(&mut engine, &code);
    assert!(
        err.to_lowercase().contains("measure") || err.contains("kilogram"),
        "expected measure/unit mismatch error, got: {err}"
    );
}

#[test]
fn fill_text_into_measure_slot_returns_planning_error() {
    let code = format!(
        r#"{INNER_SPEC}
spec almonds
uses product_structure
  -> with primary_weight: "hello"
"#
    );
    let mut engine = Engine::new();
    let err = load_err_joined(&mut engine, &code);
    assert!(
        !err.is_empty(),
        "expected planning error for text into measure, got empty"
    );
}

#[test]
fn fill_number_into_boolean_slot_returns_planning_error() {
    let code = r#"spec inner
data flag: boolean

spec outer
uses inner
  -> with flag: 10
"#;
    let mut engine = Engine::new();
    let err = load_err_joined(&mut engine, code);
    assert!(
        err.to_lowercase().contains("boolean"),
        "expected boolean mismatch error, got: {err}"
    );
}

#[test]
fn fill_measure_with_unit_succeeds() {
    let code = format!(
        r#"{INNER_SPEC}
spec almonds
uses product_structure
  -> with primary_weight: 10 kilogram
"#
    );
    let mut engine = Engine::new();
    load_ok(&mut engine, &code);
}
